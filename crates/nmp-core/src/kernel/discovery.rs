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

use super::*;
use crate::planner::{InterestScope, InterestShape};

/// Wire sub-id prefix for discovery oneshots. The EOSE handler routes any
/// sub-id with this prefix back to [`Kernel::complete_unknown_oneshot`].
pub(in crate::kernel) const ONESHOT_SUB_PREFIX: &str = "oneshot-disc-";

impl Kernel {
    /// Ingest seam: record referenced pubkeys (`p`) and event ids (`e`/`q`)
    /// from `tags` that are not already in the local projections. Borrowed
    /// predicates ⇒ no allocation when everything is known (D8).
    ///
    /// Split-borrow shape: `unknown_ids` is borrowed `&mut` while `events` /
    /// `profiles` are borrowed `&` — disjoint fields, so the caller passes
    /// `&event.tags` (no clone) from the ingest path.
    pub(in crate::kernel) fn collect_unknown_refs(&mut self, tags: &[Vec<String>]) {
        let Kernel {
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

    /// Drain the unknown-id set: register one [`OneshotApi`] interest per
    /// missing reference (forward-looking, deduped) and emit the matching M1
    /// REQ so it resolves on the wire today. Event ids → id-filtered fetch on
    /// the content relay; pubkeys → `kinds:[0]` profile fetch on the indexer.
    /// Idempotent: a second call with no intervening `collect_unknown_refs`
    /// emits nothing (the set is drained).
    pub(in crate::kernel) fn drain_unknown_oneshots(&mut self) -> Vec<OutboundMessage> {
        let (event_ids, pubkeys) = self.unknown_ids.drain();
        if event_ids.is_empty() && pubkeys.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(event_ids.len() + pubkeys.len());

        for id in event_ids {
            let shape = InterestShape {
                event_ids: [id.clone()].into_iter().collect(),
                limit: Some(1),
                ..Default::default()
            };
            let token = {
                let registry = self.lifecycle.registry_mut();
                self.oneshot
                    .request(registry, InterestScope::Global, shape)
            };
            let sub_id = format!("{ONESHOT_SUB_PREFIX}{}", token.0);
            self.oneshot_subs.insert(sub_id.clone(), token);
            out.push(self.req(
                RelayRole::Content,
                &sub_id,
                "discovery: referenced event",
                json!({ "ids": [id], "limit": 1 }),
            ));
        }
        for pk in pubkeys {
            let shape = InterestShape::profile_for(pk.clone());
            let token = {
                let registry = self.lifecycle.registry_mut();
                self.oneshot
                    .request(registry, InterestScope::Global, shape)
            };
            let sub_id = format!("{ONESHOT_SUB_PREFIX}{}", token.0);
            self.oneshot_subs.insert(sub_id.clone(), token);
            out.push(self.req(
                RelayRole::Indexer,
                &sub_id,
                "discovery: referenced profile",
                json!({ "kinds": [0], "authors": [pk], "limit": 1 }),
            ));
        }
        if !out.is_empty() {
            self.log(format!("discovery: {} oneshot fetch(es) issued", out.len()));
        }
        out
    }

    /// EOSE seam: the oneshot for `sub_id` has delivered its first stored set.
    /// Mark the token complete, then drain+release it — the registry owner is
    /// dropped (the deduped slot GCs when its last owner leaves). No-op for a
    /// non-oneshot sub-id (D6: never panics).
    pub(in crate::kernel) fn complete_unknown_oneshot(&mut self, sub_id: &str) {
        let Some(token) = self.oneshot_subs.remove(sub_id) else {
            return;
        };
        self.oneshot.complete(token);
        // `drain_completed` keeps the idempotent-drain contract; we release
        // immediately because the kernel reads results from the store/cache,
        // not from a buffered oneshot payload (PD-021 poll model).
        let _ = self.oneshot.drain_completed();
        let registry = self.lifecycle.registry_mut();
        self.oneshot.release(registry, token);
    }

    /// Count of in-flight discovery oneshots. Diagnostics/tests.
    #[cfg(test)]
    pub(in crate::kernel) fn discovery_in_flight(&self) -> usize {
        self.oneshot.in_flight()
    }
}
