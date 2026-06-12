//! Private Phase-2 advancement helpers for the W5 claim-expansion controller.
//!
//! Extracted from `claim_expansion.rs` to keep that file under the D-V12
//! 500-LOC ceiling. These functions are production code (`pub(super)`) and are
//! part of the normal build.

use crate::time::Instant;

use crate::planner::{
    HintSource, InterestId, InterestLifecycle, InterestScope, LogicalInterest, RelayHint,
};
use crate::relay::CanonicalRelayUrl;
use crate::subs::{CompileTrigger, SubIdentity, SubKey, SubOwnerKey, SubScope};

use super::{
    claim_expansion::{
        ClaimTermination, Phase, MAX_EXPANSION_CONCURRENCY, MAX_RELAYS_TRIED_PER_CLAIM,
    },
    wire_log, Kernel,
};

impl Kernel {
    /// Advance a claim to Phase 2 or fill open Phase-2 slots.
    ///
    /// Rebuilds the candidate queue, takes up to `MAX_EXPANSION_CONCURRENCY`
    /// candidates, pushes their `RelayHint`s onto the `LogicalInterest` via
    /// `registry.push()`, and enqueues a `CompileTrigger` so the planner
    /// emits the new REQs.
    pub(super) fn advance_to_phase2(&mut self, iid: InterestId, now: Instant) {
        let Some(claim) = self.pending_claims.get_mut(&iid) else {
            return;
        };

        // Lazily build/rebuild the candidate queue on each Phase-2 entry.
        // §C.E13: NIP-65 may have arrived since registration; rebuild here.
        // We need a read-only borrow of self to build the queue, but we also
        // need mutable access to update the claim. Split the work:
        let _ = now;

        let author = claim.author.clone();
        let phase = claim.phase.clone();
        let existing_attempted = claim.attempted.clone();
        let existing_queue = claim.candidate_queue.clone();

        // Build fresh candidate queue from URI hints (§8.2: Phase 2 fans out
        // through W7 hints on the existing LogicalInterest). The planner
        // already covers NIP-65 outbox relays in Phase 1; Phase 2 expands to
        // URI-provided relay hints that were not covered in Phase 1.
        let now_s = self.now_secs();
        let mut candidates: Vec<String> = existing_queue.iter().cloned().collect();
        candidates.retain(|url| !existing_attempted.contains(url));

        // Sort: descending score weight, tiebreaker lex-DESC URL (§0 Q6).
        let author_for_sort = author.clone();
        candidates.sort_by(|url_a, url_b| {
            let (wa, wb) = if let Some(ref a) = author_for_sort {
                (
                    self.relay_score_map.get(a, url_a).weight(now_s),
                    self.relay_score_map.get(a, url_b).weight(now_s),
                )
            } else {
                (0.0_f32, 0.0_f32)
            };
            wb.partial_cmp(&wa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| url_b.cmp(url_a))
        });
        candidates.dedup();

        // Count unique in-flight relays (not tuples) for concurrency limit.
        let in_flight_relay_count = {
            let mut relay_set = std::collections::BTreeSet::new();
            if let Some(claim) = self.pending_claims.get(&iid) {
                for (relay, _) in &claim.in_flight_attempts {
                    relay_set.insert(relay.clone());
                }
            }
            relay_set.len()
        };
        let open_slots = MAX_EXPANSION_CONCURRENCY.saturating_sub(in_flight_relay_count);
        let remaining_budget = MAX_RELAYS_TRIED_PER_CLAIM.saturating_sub(existing_attempted.len());
        let to_pick = open_slots.min(remaining_budget).min(candidates.len());

        if to_pick == 0 && matches!(phase, Phase::Phase1) {
            // No candidates — route through terminate_claim so claim_sub_index
            // is cleaned up (B3 invariant). Inline phase mutation would leave
            // stale reverse-index entries when poll_claim_expansion prunes later.
            self.terminate_claim(iid, ClaimTermination::Exhausted);
            return;
        }

        // Take up to `to_pick` candidates
        let picked: Vec<String> = candidates.into_iter().take(to_pick).collect();

        // Build RelayHints for the planner. URI-sourced relay hints (from the
        // NIP-19 TLV `relays` field) are represented as `UserConfigured` —
        // the closest existing variant for user-provided/publisher-provided hints.
        let hints: Vec<RelayHint> = picked
            .iter()
            .map(|url| RelayHint {
                url: url.clone(),
                source: HintSource::UserConfigured,
            })
            .collect();

        // Update claim state
        if let Some(claim) = self.pending_claims.get_mut(&iid) {
            // B5: canonicalize URLs at WRITE time into attempted set
            // (previously only canonicalized at lookup time in relay_failed).
            for url in &picked {
                let canonical = CanonicalRelayUrl::parse_or_raw(url).into_string();
                claim.attempted.insert(canonical);
            }
            // Remove picked from candidate queue
            claim.candidate_queue.retain(|url| !picked.contains(url));

            let from = match &claim.phase {
                Phase::Phase1 => "phase1",
                Phase::Phase2InFlight => "phase2",
                Phase::Terminal(_) => "terminal",
            };

            claim.phase = Phase::Phase2InFlight;

            if let Some(ref a) = author {
                wire_log::log_wire(wire_log::WireLogEvent::ClaimPhaseAdvance {
                    author: a,
                    from,
                    to: "phase2",
                    reason: "budget_elapsed",
                });
            }

            // B2: §8.2 single-LogicalInterest — update hints on the EXISTING
            // OneshotApi slot rather than creating a second registry slot.
            //
            // The claim's interest_id was returned by `OneshotApi::request` as
            // `InterestId(shape_key.0)` where `shape_key =
            // stable_hash64(("oneshot", SubScope::Global, shape))`. We
            // reconstruct the same SubIdentity and call `set_sub` (upsert) so
            // the slot's hints are replaced in-place.
            //
            // This keeps `oneshot.in_flight() == 1` across Phase 1 → Phase 2
            // because no new OneshotToken is created — only the hints change.
            let interest_id = claim.interest_id.clone();
            let shape = claim.shape.clone();
            let updated_interest = LogicalInterest {
                id: interest_id.clone(),
                scope: InterestScope::Global,
                shape: shape.clone(),
                hints,
                lifecycle: InterestLifecycle::OneShot,
                is_indexer_discovery: false,
            };
            // Reconstruct the SubIdentity that OneshotApi originally used.
            // The key is derived from (scope, shape); we must use the SAME
            // key derivation so we update the right slot (not create a new one).
            // OneshotApi uses: SubKey(stable_hash64(("oneshot", sub_scope, shape)))
            // which equals InterestId(key.0) for the returned interest_id.
            // Therefore: SubKey(interest_id.0) is the correct key.
            let sub_key = SubKey(interest_id.0);
            // The owner is per-token; using the same synthetic owner ensures we
            // update rather than add a new owner. In practice `set_sub` attaches
            // the owner AND replaces the interest — so any valid owner works here.
            let owner = SubOwnerKey::new(("claim-expansion-hint-update", interest_id.0));
            let identity = SubIdentity::new(owner, sub_key, SubScope::Global);
            self.lifecycle
                .registry_mut()
                .set_sub(identity, updated_interest);
        }

        // Trigger a planner recompile to emit the new hints as REQs (W7).
        self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened {
            interest_ids: Vec::new(),
        });
    }

    /// Mark a claim as terminal and emit a wire-log transition.
    ///
    /// B3: cleans up all `claim_sub_index` entries pointing to this claim,
    /// so the reverse index never accumulates stale entries. A debug_assert
    /// at the end verifies the index invariant.
    pub(super) fn terminate_claim(&mut self, iid: InterestId, reason: ClaimTermination) {
        let Some(claim) = self.pending_claims.get_mut(&iid) else {
            return;
        };
        let author = claim.author.clone().unwrap_or_default();
        // Cloned inside the borrow for the terminal-miss teardown below (run
        // after the borrow ends so `record_event_claim_released` can take
        // `&mut self`).
        let primary_id = claim.primary_id.clone();
        let from = match &claim.phase {
            Phase::Phase1 => "phase1",
            Phase::Phase2InFlight => "phase2",
            Phase::Terminal(_) => "terminal",
        };
        let to = match &reason {
            ClaimTermination::Hit => "terminal_hit",
            ClaimTermination::Exhausted => "terminal_exhausted",
            ClaimTermination::Budget => "terminal_budget",
        };
        // Compute the terminal-miss decision BEFORE `reason` is moved into
        // `Phase::Terminal(reason)` below (ClaimTermination is not `Copy`).
        let is_terminal_miss = matches!(
            reason,
            ClaimTermination::Exhausted | ClaimTermination::Budget
        );
        wire_log::log_wire(wire_log::WireLogEvent::ClaimPhaseAdvance {
            author: &author,
            from,
            to,
            reason: to,
        });
        claim.phase = Phase::Terminal(reason);

        // B3: remove all reverse-index entries pointing to this claim
        self.claim_sub_index.retain(|_, v| *v != iid);

        // V-59 rung 1 (#4) — terminal-miss teardown. Released here (the single
        // controller-owned termination site) ONLY for the two genuine
        // no-event outcomes:
        //   - `Exhausted`: every candidate relay was tried and none had it.
        //   - `Budget`:    the total per-claim budget elapsed first.
        // In both cases the relay set has confirmed (or timed out trying to
        // confirm) that no relay holds the event, so we clear the claim state
        // (`event_claims` refcount row + `event_claim_requested`) and push the
        // id into the release ring so a re-claim re-fetches. A `Hit` MUST keep
        // the `event_claims` row intact — the matching EVENT is now in the
        // store and the `claimed_events` projection surfaces it on the next
        // snapshot tick. (Previously this teardown lived in
        // `complete_unknown_oneshot` and fired on the FIRST relay's
        // EOSE-no-match, racing a sibling relay's still-in-flight EVENT.)
        if is_terminal_miss {
            self.event_claims.remove(&primary_id);
            self.event_claim_requested.remove(&primary_id);
            self.record_event_claim_released(&primary_id);
        }

        // B3 invariant: every remaining claim_sub_index value must point to
        // an existing pending_claim (after terminal entries are removed in the
        // caller's retain pass, this holds; here we assert the forward direction).
        debug_assert!(
            self.claim_sub_index
                .values()
                .all(|id| self.pending_claims.contains_key(id)),
            "claim_sub_index drift: some entries point to non-existent claims"
        );
    }
}
