//! Unknown-id discovery seam — the kernel side of T82 (`docs/design/
//! nostrdb-notedeck-lessons.md` §3.9 + §3.10).
//!
//! Three narrow entry points keep the kernel change reviewable:
//! - [`Kernel::collect_unknown_refs`] — called from ingest right after an
//!   event is persisted; feeds referenced-but-missing ids (`p`/`e`/`q` tags)
//!   into [`crate::subs::UnknownIds`] using the **borrowed visitor** (D8:
//!   zero per-event allocation when every reference is already cached).
//! - [`Kernel::collect_content_mention_pubkeys`] — called from ingest for
//!   note kinds (kind:1 etc.) immediately after `collect_unknown_refs`. Scans
//!   `event.content` for `nostr:npub1…` / `nostr:nprofile1…` URIs that appear
//!   **only** in the content body (no matching `p`-tag) and feeds their pubkeys
//!   into `UnknownIds` via the same `note_pubkey` path. D8-clean: the
//!   `nostr:` substring guard short-circuits before any allocation on the
//!   common path (content with no mentions). Implementor note: `nmp-content`
//!   depends on `nmp-core` so importing the full tokenizer here would create a
//!   dep cycle; the minimal `nostr:` URI extractor below reuses the existing
//!   `parse_nostr_uri` free function that already lives in `nmp_core::nip21`.
//! - [`Kernel::drain_unknown_oneshots`] — turns the deduped unknown set into
//!   [`crate::subs::OneshotApi`] requests on the lifecycle's registry, AND
//!   enqueues a [`crate::subs::CompileTrigger::ViewOpened`] so the planner's
//!   next `drain_tick` (driven from the actor idle loop via
//!   [`Kernel::drain_lifecycle_tick`]) compiles those interests into wire
//!   frames. The trigger enqueue is load-bearing: without it, `drain_tick`
//!   short-circuits on an empty inbox and the discovery REQ never reaches the
//!   wire on a tick where no other compile trigger (FollowListChanged,
//!   Nip65Arrived, …) happens to be flowing. Pre-PD-033-C the M1
//!   `self.req(...)` dual-write masked this gap; Stage 1 retires the dual-write
//!   so the trigger enqueue is now the sole driver. Cold-start routing for
//!   `OneShot + Global + event_ids` / `… + authors` is handled by the
//!   planner's `bootstrap_content_relays` and `bootstrap_indexer_relays`
//!   lanes (PR #365 planner extension). Called from `pending_view_requests`.
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

use super::{Kernel, OutboundMessage};
use crate::nip21::{parse_nostr_uri, NostrUri};
use crate::planner::{InterestScope, InterestShape};
use crate::subs::CompileTrigger;

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

    /// Ingest seam (V-56): record profile pubkeys mentioned in `content` as
    /// `nostr:npub1…` / `nostr:nprofile1…` URIs that do **not** appear in a
    /// `p`-tag. These content-only mentions would otherwise render indefinitely
    /// without a kind:0 fetch.
    ///
    /// D8: the `content.contains("nostr:")` guard exits before any allocation
    /// on the common path (notes without `nostr:` URIs). When mentions are
    /// present the only allocations are `to_string()` calls for pubkeys that
    /// are genuinely missing from both `profiles` and the pending `unknown_ids`
    /// set — the same cost as a `p`-tag hit in `collect_unknown_refs`.
    ///
    /// D6: parse failures are silently skipped (no panic, no `Result`).
    ///
    /// Implementation note: `nmp-content` depends on `nmp-core`, so reusing
    /// the full regex tokenizer would create a dep cycle. The hand-rolled
    /// scanner below reuses `parse_nostr_uri` (already in `nmp_core::nip21`)
    /// and splits on ASCII whitespace / common delimiters to isolate tokens —
    /// the same set of surface tokens the tokenizer's regex matches.
    pub(in crate::kernel) fn collect_content_mention_pubkeys(&mut self, content: &str) {
        // Fast-path: most notes contain no `nostr:` URIs. A single `contains`
        // is O(n) but avoids all allocation and the borrow split below.
        if !content.contains("nostr:") {
            return;
        }

        // Split-borrow: `unknown_ids` is `&mut` while `profiles` is `&`.
        let Self {
            unknown_ids,
            profiles,
            ..
        } = self;

        // Tokenise on whitespace and common delimiters that can surround a
        // `nostr:` URI in plain text (parentheses, commas, angle-brackets,
        // newlines, zero-width joiners are irrelevant at the byte level because
        // bech32 is alphanumeric only — splitting on these never truncates a
        // valid bech32 string).
        for raw in content.split(|c: char| {
            c.is_ascii_whitespace() || matches!(c, ',' | '(' | ')' | '"' | '\'' | '<' | '>')
        }) {
            // Only try to parse tokens that look like a nostr: URI.  The
            // leading 10 characters `nostr:npub1` / `nostr:npro` are the
            // only profile-bearing prefixes we care about.
            if !raw.starts_with("nostr:npub1") && !raw.starts_with("nostr:nprofile1") {
                continue;
            }
            // Strip any trailing punctuation that is not valid bech32
            // (e.g. the `.` in "...see nostr:npub1xxx.").
            let trimmed = raw.trim_end_matches(|c: char| !c.is_alphanumeric());
            match parse_nostr_uri(trimmed) {
                Ok(NostrUri::Profile { pubkey, .. }) => {
                    unknown_ids.note_pubkey(&pubkey, |pk| profiles.contains_key(pk));
                }
                // Event / Address refs in content are already covered by
                // collect_unknown_refs via `e`/`q` tags (Article VII: no
                // speculative future-proofing).
                Ok(NostrUri::Event { .. } | NostrUri::Address { .. }) => {}
                Err(_) => {}
            }
        }
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
        //
        // PD-033-C Stage 1: the cap is now measured against the OneshotApi's
        // registered (pending) count — `oneshot_subs` is no longer populated
        // here (the planner-emitted `sub_id` lands there via the
        // `register_planner_wire_frames` bridge), so it would under-count on
        // a tick where interests are registered but the planner has not yet
        // compiled their REQ frames. `oneshot.in_flight()` is the authoritative
        // pending count; an entry leaves it only when `complete_unknown_oneshot`
        // calls `oneshot.release(...)`.
        let in_flight = self.oneshot.in_flight();
        if in_flight >= Self::MAX_DISCOVERY_CONCURRENCY {
            return Vec::new();
        }
        let slots = Self::MAX_DISCOVERY_CONCURRENCY - in_flight;

        let (event_ids, pubkeys) = self.unknown_ids.drain();
        if event_ids.is_empty() && pubkeys.is_empty() {
            return Vec::new();
        }

        // PD-033-C Stage 1: this function no longer emits `OutboundMessage`
        // frames directly. The M1 `self.req(...)` dual-writes were retired in
        // both arms below; the canonical wire-frame emission now flows through
        // the planner's `drain_tick` (called from `Kernel::drain_lifecycle_tick`
        // on the actor idle loop). The `Vec<OutboundMessage>` return type is
        // retained so the `pending_view_requests` `requests.extend(...)` shape
        // stays unchanged; subsequent PD-033-C stages may delete the return
        // value once every caller is migrated.
        let mut slots_used = 0usize;

        // Track whether at least one new oneshot was registered this drain.
        // If so, we enqueue a single coalesced `ViewOpened` trigger at the end
        // so the planner's next `drain_tick` (driven by the actor idle loop)
        // compiles the newly-registered interests into WireFrames. Without
        // this enqueue the registry would carry the interest but `drain_tick`
        // would short-circuit on an empty inbox — the discovery REQ would
        // never make it onto the wire on a cold-start tick where no other
        // trigger (FollowListChanged, Nip65Arrived, …) happens to be flowing.
        // Pre-PD-033-C the M1 `self.req(...)` dual-write masked this gap;
        // Stage 1 retires the dual-write so the trigger enqueue is now
        // load-bearing.
        let mut registered_any = false;

        // Events sub (content relay) — take first batch, put back the rest.
        if !event_ids.is_empty() && slots_used < slots {
            let (batch, remainder) = event_ids.split_at(event_ids.len().min(Self::DISCOVERY_BATCH));
            let shape = InterestShape {
                event_ids: batch.iter().cloned().collect(),
                limit: Some(batch.len() as u32),
                ..Default::default()
            };
            let (token, interest_id) = {
                let registry = self.lifecycle.registry_mut();
                self.oneshot
                    .request(registry, InterestScope::Global, shape, Vec::new())
            };
            // PD-033-C Stage 1 bridge: stash the token by interest_id. The
            // planner's next `drain_tick` emits a `WireFrame::Req` carrying
            // this `interest_id`; `register_planner_wire_frames` consumes
            // the pending entry and inserts `oneshot_subs` keyed by the
            // planner-assigned `sub_id` so EOSE / store-gate routing works
            // against the actual wire sub-id.
            self.pending_discovery_oneshots.insert(interest_id, token);
            registered_any = true;
            // Cold-start routing for `OneShot + Global + event_ids` is handled
            // by the planner's `bootstrap_content_relays` lane (PR #365); see
            // `crates/nmp-core/src/planner/compiler/partition/mod.rs` Case D
            // head check.
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
                kinds: [
                    crate::kinds::KIND_PROFILE_METADATA,
                    crate::kinds::KIND_CONTACT_LIST,
                    crate::kinds::KIND_RELAY_LIST,
                ]
                .into_iter()
                .collect(),
                limit: Some(batch.len() as u32 * 3),
                ..Default::default()
            };
            let (token, interest_id) = {
                let registry = self.lifecycle.registry_mut();
                self.oneshot
                    .request(registry, InterestScope::Global, shape, Vec::new())
            };
            // PD-033-C Stage 1 bridge (see twin comment in events arm).
            self.pending_discovery_oneshots.insert(interest_id, token);
            registered_any = true;
            // Cold-start routing for the profile shape (`OneShot + Global +
            // authors`, no NIP-65 mailbox) is handled by the planner's
            // `bootstrap_indexer_relays` fallback (PR #365); see
            // `crates/nmp-core/src/planner/compiler/partition/case_a_authors.rs`.
            if !remainder.is_empty() {
                self.unknown_ids.put_back_pubkeys(remainder.iter().cloned());
            }
        } else if !pubkeys.is_empty() {
            self.unknown_ids.put_back_pubkeys(pubkeys);
        }

        if registered_any {
            // A2 — view-equivalent registered one or more interests. The
            // `interest_ids` field is diagnostic provenance only (the
            // compiler walks the full registry, not a filtered subset), so
            // an empty Vec is a correct and zero-allocation form. Per-tick
            // coalescing in the trigger inbox guarantees ≤1 recompile per
            // tick regardless of how many oneshots this drain registered.
            self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened {
                interest_ids: Vec::new(),
            });
        }

        Vec::new()
    }

    /// EOSE seam: the oneshot for `sub_id` has delivered its first stored set.
    /// Mark the token complete, then drain+release it — the registry owner is
    /// dropped (the deduped slot GCs when its last owner leaves). No-op for a
    /// non-oneshot sub-id (D6: never panics).
    pub(in crate::kernel) fn complete_unknown_oneshot(&mut self, sub_id: &str) {
        let Some((token, _kind)) = self.oneshot_subs.remove(sub_id) else {
            return;
        };

        // V-59 rung 1 (#4) — EOSE handling for an event claim is NO LONGER
        // released here.
        //
        // A claim's REQ fans out to MULTIPLE relays sharing one `sub_id`
        // (B4 shape-shared subs). A single relay's EOSE-no-match means only
        // THAT relay had nothing — a sibling relay's matching EVENT may still
        // be in flight. Releasing the claim on the first EOSE here raced the
        // EVENT: the relay set's slowest member could deliver the event AFTER
        // a faster, content-less relay EOSE'd, and the claim row would already
        // be gone — the `claimed_events` projection would then never surface
        // the stored event (the embed renders "loading" forever).
        //
        // The per-relay EOSE is recorded by `record_claim_expansion_eose_no_match`
        // (called immediately after this in the EOSE arm), which removes this
        // relay's `in_flight_attempts` slot. The claim is released ONLY when the
        // controller (`poll_claim_expansion`) observes genuine terminal-miss —
        // `Terminal(Exhausted)` (all candidate relays tried, none in flight) or
        // `Terminal(Budget)` (total budget elapsed). The `event_claims` teardown
        // + release-ring fan-out lives in `terminate_claim`, gated on those two
        // reasons (a `Hit` keeps the row so the projection surfaces the event).
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
