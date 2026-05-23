//! Unknown-id discovery seam — the kernel side of T82 (`docs/design/
//! nostrdb-notedeck-lessons.md` §3.9 + §3.10).
//!
//! Three narrow entry points keep the kernel change reviewable:
//! - [`Kernel::collect_unknown_refs`] — called from ingest right after an
//!   event is persisted; feeds referenced-but-missing ids into
//!   [`crate::subs::UnknownIds`] using the **borrowed visitor** (D8: zero
//!   per-event allocation when every reference is already cached).
//! - [`Kernel::drain_unknown_oneshots`] — turns the deduped unknown set into
//!   [`crate::subs::OneshotApi`] requests on the lifecycle's registry **and**
//!   emits the matching M1 REQ frames so discovery actually resolves on the
//!   wire (the lifecycle wire-emitter is dormant in the kernel per
//!   `kernel/mod.rs`; the oneshot registry registration is the forward-looking
//!   half, the `req()` emission is what fetches today). Called from
//!   `pending_view_requests`.
//! - [`Kernel::complete_unknown_oneshot`] — called from the EOSE handler; the
//!   `OneShot` lifecycle means "first stored-set delivered" == EOSE, so the
//!   token completes there and the registry owner is released (slot GCs when
//!   no other deduped oneshot holds it).
//!
//! "Known" is judged against the kernel's in-memory projections (`events` /
//! `profiles`) — the same caches the rest of the kernel treats as
//! authoritative-for-render. Borrowed `&str` predicates ⇒ no allocation on
//! the hot path (D8). The actor owns all this state; nothing crosses FFI and
//! no `Result` is produced (D6).

use super::{json, Kernel, OutboundMessage, RelayRole};
use crate::planner::{InterestScope, InterestShape};

/// Typed discriminant for entries in [`Kernel::oneshot_subs`].
///
/// Replaces the `"oneshot-disc-"` string-prefix routing that previously
/// required callers to call `sub_id.starts_with(ONESHOT_SUB_PREFIX)` to
/// determine how to handle a completed oneshot. Adding a new oneshot kind
/// now requires extending this enum; the compiler enforces exhaustive
/// handling wherever the variant is matched.
///
/// Today only `Discovery` exists. Profile-claim and thread-hydration subs use
/// different sub-id schemes and are NOT stored in `oneshot_subs`; adding
/// spurious variants here would be speculative future-proofing (Article VII).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::kernel) enum OneshotKind {
    /// An id-or-pubkey discovery fetch issued by [`Kernel::drain_unknown_oneshots`].
    Discovery,
}

/// Wire sub-id prefix for discovery oneshots. Retained for sub-id
/// construction (the prefix makes wire logs readable); routing is done
/// via [`OneshotKind`], not via `starts_with` on this constant.
pub(in crate::kernel) const ONESHOT_SUB_PREFIX: &str = "oneshot-disc-";

impl Kernel {
    /// Max ids/pubkeys per discovery REQ. Relays that reject large id-filters
    /// gracefully drop events; keeping this ≤50 is conservative but safe.
    const DISCOVERY_BATCH: usize = 50;

    /// Maximum concurrent discovery REQs across both relay roles. Keeps us
    /// well under relay concurrent-sub limits (~15-20 on most public relays)
    /// even during startup bursts that accumulate thousands of unknown refs.
    /// The remainder is held in `unknown_ids` and drained on subsequent ticks
    /// as in-flight subs close via EOSE.
    const MAX_DISCOVERY_CONCURRENCY: usize = 2;

    /// Ingest seam: record referenced pubkeys (`p`) and event ids (`e`/`q`)
    /// from `tags` that are not already in the local projections. Borrowed
    /// predicates ⇒ no allocation when everything is known (D8).
    ///
    /// Split-borrow shape: `unknown_ids` is borrowed `&mut` while `events` /
    /// `profiles` are borrowed `&` — disjoint fields, so the caller passes
    /// `&event.tags` (no clone) from the ingest path.
    pub(in crate::kernel) fn collect_unknown_refs(&mut self, tags: &[Vec<String>]) {
        let Self {
            unknown_ids,
            events,
            profiles,
            ..
        } = self;
        unknown_ids.visit_tags(
            tags,
            |id| events.contains_key(id),
            |pk| profiles.contains_key(pk),
        );
    }

    /// Drain the unknown-id set up to [`Self::MAX_DISCOVERY_CONCURRENCY`]
    /// concurrent REQs. Each REQ carries up to [`Self::DISCOVERY_BATCH`] ids.
    /// Remaining unknown refs are put back into `unknown_ids` and will be
    /// drained on the next tick once in-flight subs close via EOSE.
    ///
    /// Idempotent: a second call with no intervening `collect_unknown_refs`
    /// emits nothing (the set is drained or at the concurrency cap).
    pub(in crate::kernel) fn drain_unknown_oneshots(&mut self) -> Vec<OutboundMessage> {
        // Respect the concurrency cap — relay NOTICE "too many concurrent REQs"
        // was the original bug (T82). Don't open more discovery subs until
        // existing ones close via EOSE.
        let in_flight = self.oneshot_subs.len();
        if in_flight >= Self::MAX_DISCOVERY_CONCURRENCY {
            return Vec::new();
        }
        let slots = Self::MAX_DISCOVERY_CONCURRENCY - in_flight;

        let (event_ids, pubkeys) = self.unknown_ids.drain();
        if event_ids.is_empty() && pubkeys.is_empty() {
            return Vec::new();
        }

        let mut out = Vec::with_capacity(slots.min(2));
        let mut slots_used = 0usize;

        // Events sub (content relay) — take first batch, put back the rest.
        if !event_ids.is_empty() && slots_used < slots {
            let (batch, remainder) = event_ids.split_at(event_ids.len().min(Self::DISCOVERY_BATCH));
            let shape = InterestShape {
                event_ids: batch.iter().cloned().collect(),
                limit: Some(batch.len() as u32),
                ..Default::default()
            };
            let token = {
                let registry = self.lifecycle.registry_mut();
                self.oneshot.request(registry, InterestScope::Global, shape)
            };
            let sub_id = format!("{ONESHOT_SUB_PREFIX}{}", token.0);
            self.oneshot_subs.insert(sub_id.clone(), (token, OneshotKind::Discovery));
            // TODO(pd033c-stage1): dual-write D4 violation — `oneshot.request`
            // above already registers this interest in `InterestRegistry`
            // (System #2). The `self.req(...)` below ALSO writes the same fact
            // into `Kernel.wire.subs` (System #1, via the M1 helper). Stage 1
            // of the PD-033-C migration deletes this `self.req(...)` so the
            // planner's next `drain_tick` emits the WireFrame instead. See
            // `docs/architecture-audit/pd033c-plan.md` §1.3 and §5 Stage 1.
            out.extend(self.req(
                RelayRole::Content,
                &sub_id,
                "discovery: referenced events",
                json!({ "ids": batch, "limit": batch.len() }),
            ));
            if !remainder.is_empty() {
                self.unknown_ids.put_back_events(remainder.iter().cloned());
            }
            slots_used += 1;
        } else if !event_ids.is_empty() {
            // No slot available; put everything back.
            self.unknown_ids.put_back_events(event_ids);
        }

        // Profiles sub (indexer) — same pattern.
        if !pubkeys.is_empty() && slots_used < slots {
            let (batch, remainder) = pubkeys.split_at(pubkeys.len().min(Self::DISCOVERY_BATCH));
            let shape = InterestShape {
                authors: batch.iter().cloned().collect(),
                kinds: [0u32, 3, 10002].into_iter().collect(),
                limit: Some(batch.len() as u32 * 3),
                ..Default::default()
            };
            let token = {
                let registry = self.lifecycle.registry_mut();
                self.oneshot.request(registry, InterestScope::Global, shape)
            };
            let sub_id = format!("{ONESHOT_SUB_PREFIX}{}", token.0);
            self.oneshot_subs.insert(sub_id.clone(), (token, OneshotKind::Discovery));
            // TODO(pd033c-stage1): dual-write D4 violation — see twin TODO in
            // the events-oneshot arm above. Stage 1 deletes this `self.req(...)`
            // call; the `oneshot.request(...)` two lines up is already the
            // canonical (InterestRegistry) registration.
            out.extend(self.req(
                RelayRole::Indexer,
                &sub_id,
                "discovery: referenced profiles",
                json!({ "kinds": [0, 3, 10002], "authors": batch, "limit": batch.len() * 3 }),
            ));
            if !remainder.is_empty() {
                self.unknown_ids.put_back_pubkeys(remainder.iter().cloned());
            }
        } else if !pubkeys.is_empty() {
            self.unknown_ids.put_back_pubkeys(pubkeys);
        }

        if !out.is_empty() {
            self.log(format!(
                "discovery: {} REQ(s) issued ({} in-flight after, {} queued)",
                out.len(),
                self.oneshot_subs.len(),
                self.unknown_ids.pending_len(),
            ));
        }
        out
    }

    /// EOSE seam: the oneshot for `sub_id` has delivered its first stored set.
    /// Mark the token complete, then drain+release it — the registry owner is
    /// dropped (the deduped slot GCs when its last owner leaves). No-op for a
    /// non-oneshot sub-id (D6: never panics).
    pub(in crate::kernel) fn complete_unknown_oneshot(&mut self, sub_id: &str) {
        let Some((token, _kind)) = self.oneshot_subs.remove(sub_id) else {
            return;
        };
        self.oneshot.complete(token);
        // `drain_completed` keeps the idempotent-drain contract; we release
        // immediately because the kernel reads results from the store/cache,
        // not from a buffered oneshot payload (idempotent poll model).
        let _ = self.oneshot.drain_completed();
        let registry = self.lifecycle.registry_mut();
        self.oneshot.release(registry, token);
    }

    /// Returns `true` if `sub_id` is a registered discovery oneshot.
    ///
    /// Callers that previously used `sub_id.starts_with(ONESHOT_SUB_PREFIX)`
    /// to route EOSE / store-gate decisions should use this instead — the
    /// `HashMap` lookup is O(1) and the routing decision is made on the typed
    /// [`OneshotKind`] stored alongside the token, not on a string prefix.
    pub(in crate::kernel) fn is_discovery_oneshot(&self, sub_id: &str) -> bool {
        matches!(
            self.oneshot_subs.get(sub_id),
            Some((_, OneshotKind::Discovery))
        )
    }

    /// Count of in-flight discovery oneshots. Diagnostics/tests.
    #[cfg(test)]
    pub(in crate::kernel) fn discovery_in_flight(&self) -> usize {
        self.oneshot.in_flight()
    }
}
