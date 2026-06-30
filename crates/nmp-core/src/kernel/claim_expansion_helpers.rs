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

use super::{
    claim_expansion::{
        ClaimTermination, Phase, MAX_EXPANSION_CONCURRENCY, MAX_RELAYS_TRIED_PER_CLAIM,
    },
    wire_log, Kernel,
};

impl Kernel {
    /// Release the OneshotApi owner backing an event-claim interest.
    ///
    /// Event claims keep their one-shot registry owner after the first relay
    /// EOSE so Phase 2 can replace hints on the same interest. This helper is
    /// the claim-owned teardown site for Hit, terminal miss, and explicit ref
    /// release. It also clears any pre-wire bridge entry so a released claim
    /// cannot be resurrected by a later planner frame.
    pub(super) fn release_claim_oneshot_owner(&mut self, interest_id: &InterestId) {
        let token = self
            .pending_discovery_oneshots
            .remove(interest_id)
            .or_else(|| self.oneshot.token_for_interest(interest_id));
        let Some(token) = token else {
            return;
        };

        self.oneshot_subs
            .retain(|_, (registered_token, _)| *registered_token != token);
        let registry = self.lifecycle.registry_mut();
        let _ = self.oneshot.release(registry, token);
    }

    /// Advance a claim to Phase 2 or fill open Phase-2 slots.
    ///
    /// Rebuilds the candidate queue, takes up to `MAX_EXPANSION_CONCURRENCY`
    /// candidates, replaces the existing one-shot interest hints through the
    /// unified registration front door, and enqueues a `CompileTrigger` so the
    /// planner emits the new REQs.
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
        let interest_id = claim.interest_id.clone();
        let shape = claim.shape.clone();
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

        let Some(identity) = self.oneshot.identity_for_interest(&interest_id) else {
            return;
        };

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
            // `identity_for_interest` returns the same owner+key+scope triple
            // minted by `OneshotApi::prepare`, so `Replace` mutates the in-flight
            // one-shot instead of attaching a second owner.
            //
            // This keeps `oneshot.in_flight() == 1` across Phase 1 → Phase 2
            // because no new OneshotToken is created — only the hints change.
            let updated_interest = LogicalInterest {
                id: interest_id.clone(),
                scope: InterestScope::Global,
                shape: shape.clone(),
                hints,
                lifecycle: InterestLifecycle::OneShot,
                is_indexer_discovery: false,
            };
            // Unified front-door (Replace = set_sub semantics): replaces the
            // hint in place. Hints differ ⇒ plan_relevant_change == true ⇒
            // recompile fires (emits W7 hint REQs); shape unchanged ⇒ same
            // completion key ⇒ serve is an idempotent no-op (safe; §5 safety).
            self.register_interest(
                &[crate::kernel::cache_serve::InterestRegistration {
                    identity,
                    interest: updated_interest,
                    policy: crate::kernel::cache_serve::InterestWrite::Replace,
                }],
                "claim-expansion-phase2",
            );
        }
    }

    /// Mark a claim as terminal and emit a wire-log transition.
    ///
    /// B3: cleans up all `claim_sub_index` entries pointing to this claim,
    /// so the reverse index never accumulates stale entries. A debug_assert
    /// at the end verifies the index invariant.
    pub(super) fn terminate_claim(&mut self, iid: InterestId, reason: ClaimTermination) {
        let (primary_id, interest_id, is_terminal_miss) = {
            let Some(claim) = self.pending_claims.get_mut(&iid) else {
                return;
            };
            let author = claim.author.clone().unwrap_or_default();
            let primary_id = claim.primary_id.clone();
            let interest_id = claim.interest_id.clone();
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
            (primary_id, interest_id, is_terminal_miss)
        };

        self.release_claim_oneshot_owner(&interest_id);

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
        // store and `refs.event` surfaces it on the next snapshot tick.
        // (Previously this teardown lived in
        // `complete_unknown_oneshot` and fired on the FIRST relay's
        // EOSE-no-match, racing a sibling relay's still-in-flight EVENT.)
        if is_terminal_miss {
            // BLOCKING 1 — route terminal-miss teardown through the SINGLE unified
            // `teardown_event_claim_key` shared with `release_event_ref`. Removing
            // `event_claims` / `event_claim_requested` directly here (the old code)
            // bypassed the shape-map + per-key-rev cleanup that lives only in the
            // release path, leaving `ref_event_shapes` + `event_row_revs` live for a
            // deleted claim (D4 second-writer; resurrects a stale ref row). The
            // unified fn drops every per-key field (refcount, dedup marker, shape
            // map, Live slot, claim-expansion tracker) AND clears the per-key rev to
            // its final value in one place, so neither path can diverge.
            self.teardown_event_claim_key(&primary_id);
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
