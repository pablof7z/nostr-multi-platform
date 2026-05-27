//! Private Phase-2 advancement helpers for the W5 claim-expansion controller.
//!
//! Extracted from `claim_expansion.rs` to keep that file under the D-V12
//! 500-LOC ceiling. These functions are production code (`pub(super)`) and are
//! part of the normal build.

use std::time::Instant;

use crate::planner::{
    HintSource, InterestId, InterestLifecycle, InterestScope, LogicalInterest, RelayHint,
};
use crate::subs::CompileTrigger;

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
        let existing_in_flight = claim.in_flight_subs.len();

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

        // How many open slots?
        let open_slots = MAX_EXPANSION_CONCURRENCY.saturating_sub(existing_in_flight);
        let remaining_budget = MAX_RELAYS_TRIED_PER_CLAIM.saturating_sub(existing_attempted.len());
        let to_pick = open_slots.min(remaining_budget).min(candidates.len());

        if to_pick == 0 && matches!(phase, Phase::Phase1) {
            // No candidates available — immediately terminate as Exhausted
            if let Some(claim) = self.pending_claims.get_mut(&iid) {
                claim.phase = Phase::Terminal(ClaimTermination::Exhausted);
                if let Some(ref a) = author {
                    wire_log::log_wire(wire_log::WireLogEvent::ClaimPhaseAdvance {
                        author: a,
                        from: "phase1",
                        to: "terminal_exhausted",
                        reason: "no_candidates",
                    });
                }
            }
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
            // Mark picked relays as attempted
            for url in &picked {
                claim.attempted.insert(url.clone());
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

            // Re-push the LogicalInterest with updated hints so the planner
            // emits the new REQs. §8.2: `registry.push()` upserts by id.
            let updated_interest = LogicalInterest {
                id: claim.interest_id.clone(),
                scope: InterestScope::Global,
                shape: claim.shape.clone(),
                hints,
                lifecycle: InterestLifecycle::OneShot,
            };
            self.lifecycle.registry_mut().push(updated_interest);
        }

        // Trigger a planner recompile to emit the new hints as REQs (W7).
        self.lifecycle.enqueue_trigger(CompileTrigger::ViewOpened {
            interest_ids: Vec::new(),
        });
    }

    /// Mark a claim as terminal and emit a wire-log transition.
    pub(super) fn terminate_claim(&mut self, iid: InterestId, reason: ClaimTermination) {
        let Some(claim) = self.pending_claims.get_mut(&iid) else {
            return;
        };
        let author = claim.author.clone().unwrap_or_default();
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
        wire_log::log_wire(wire_log::WireLogEvent::ClaimPhaseAdvance {
            author: &author,
            from,
            to,
            reason: to,
        });
        claim.phase = Phase::Terminal(reason);
    }
}
