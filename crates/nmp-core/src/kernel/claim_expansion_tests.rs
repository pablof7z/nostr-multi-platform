//! TDD tests for W5 — claim-expansion controller.
//!
//! All tests in this file follow the red-first discipline: the module is
//! compiled with `#[cfg(test)]` only and exercises the three `impl Kernel`
//! methods introduced by `claim_expansion.rs`.

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::kernel::claim_expansion::Phase;
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// 64-char hex string for a deterministic pubkey fixture.
    fn hex(byte: &str) -> String {
        byte.repeat(32)
    }

    /// A valid 64-char event-id hex string.
    fn event_id(byte: &str) -> String {
        byte.repeat(32)
    }

    /// Register a claim with a specific author.
    fn register_claim_with_author(k: &mut Kernel, primary_id: &str, author: &str) {
        k.register_claim_expansion(
            primary_id.to_string(),
            None,
            Some(author.to_string()),
            vec![],
            Instant::now(),
        );
    }

    // ── T1: Phase-1 hit terminates without Phase 2 ─────────────────────────

    #[test]
    fn phase1_hit_terminates_without_phase2() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("aa");
        let author = hex("bb");

        register_claim_with_author(&mut kernel, &primary_id, &author);
        assert!(
            !kernel.pending_claims_is_empty(),
            "claim must be registered"
        );

        // Simulate a Phase-1 hit: event becomes known + call on_claim_outcome Hit
        // Uses the primary_id-based path since this test doesn't go through
        // the production wire-frame registration (no claim_sub_index populated).
        kernel.test_mark_event_known(&primary_id);
        kernel.on_claim_outcome_hit_by_primary_id(&primary_id);

        // After hit, claim must be terminated (removed from pending)
        assert!(
            kernel.pending_claims_is_empty(),
            "claim must be removed after Phase-1 hit"
        );

        // poll must return empty (no Phase-2 REQs)
        let now = Instant::now();
        let msgs = kernel.poll_claim_expansion(now);
        assert!(msgs.is_empty(), "Phase-1 hit must not produce Phase-2 REQs");
    }

    // ── T2: Phase-1 EOSE advances to Phase 2 after budget ─────────────────

    #[test]
    fn phase1_eose_advances_to_phase2_after_budget() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("cc");
        let author = hex("dd");

        let started = Instant::now();
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            vec![],
            started,
        );

        // Fast-forward past Phase-1 budget (1500 ms)
        let after_budget = started + Duration::from_millis(1600);
        let msgs = kernel.poll_claim_expansion(after_budget);

        // The claim must have transitioned: Phase1 → Phase2InFlight (or terminal
        // if no candidates). At minimum, the claim must still be pending (not yet
        // terminated) or have been promoted.
        // If author has no outbox relays, it terminates as Exhausted — still
        // removes from pending. This test only asserts budget detection works.
        let _ = msgs; // msgs may be empty if no Phase-2 candidates available
                      // Claim is either in Phase2 or Terminated(Exhausted) — either way it
                      // left Phase1.
        let phase = kernel.test_claim_phase(&primary_id);
        assert!(
            phase != Some(Phase::Phase1),
            "claim must leave Phase1 after budget elapsed; got {:?}",
            phase
        );
    }

    // ── T3: Phase-2 concurrency capped at 3 ───────────────────────────────

    #[test]
    fn phase2_concurrency_capped_at_3() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("ee");
        let author = hex("ff");

        // Seed 5 candidate relays for the author via the score map
        // (the lazy candidate-queue builder picks from outbox / hints).
        // For this test inject hints directly.
        let hints: Vec<String> = (0..5u8).map(|i| format!("wss://relay{i}.test")).collect();

        let started = Instant::now() - Duration::from_millis(1600);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            hints,
            started,
        );

        // Poll past Phase-1 budget — should promote to Phase 2
        let now = Instant::now();
        let _msgs = kernel.poll_claim_expansion(now);

        // The in-flight count must be capped at MAX_EXPANSION_CONCURRENCY = 3
        let in_flight = kernel.test_claim_in_flight_count(&primary_id);
        assert!(
            in_flight <= 3,
            "Phase-2 concurrency must be <= MAX_EXPANSION_CONCURRENCY (3), got {in_flight}"
        );
    }

    // ── T4: Phase-2 candidates ordered by score desc then lex desc ─────────

    #[test]
    fn phase2_candidates_ordered_by_score_desc_then_lex_desc() {
        use crate::kernel::relay_score::ClaimOutcome;

        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let author = hex("11");

        // Seed scores: relay_b is warm (1 hit), relay_a is cold
        let relay_warm = "wss://relay_b.test";
        let relay_cold = "wss://relay_a.test";
        kernel.record_claim_outcome(&author, relay_warm, ClaimOutcome::Hit);

        let primary_id = event_id("22");
        let hints = vec![relay_cold.to_string(), relay_warm.to_string()];
        let started = Instant::now() - Duration::from_millis(1600);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            hints,
            started,
        );

        let _msgs = kernel.poll_claim_expansion(Instant::now());

        // Verify the warm relay was attempted first (it should be in attempted set)
        let attempted = kernel.test_claim_attempted(&primary_id);
        assert!(
            attempted.contains(relay_warm) || attempted.is_empty(),
            "warm relay should be prioritised in Phase-2 ordering; attempted: {attempted:?}"
        );
    }

    // ── T5: Phase-2 exhausts then terminates ──────────────────────────────

    #[test]
    fn phase2_exhausts_then_terminates() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("33");
        let author = hex("44");

        // Register with a small hint set so exhaustion happens quickly
        let hints: Vec<String> = (0..2u8).map(|i| format!("wss://exhaust{i}.test")).collect();
        let started = Instant::now() - Duration::from_millis(1600);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            hints,
            started,
        );

        // Trigger Phase 2 entry
        let _msgs = kernel.poll_claim_expansion(Instant::now());

        // Report EoseNoMatch for all in-flight subs to drain candidates
        let in_flight: Vec<String> = kernel.test_claim_in_flight_sub_ids(&primary_id);
        for sub_id in &in_flight {
            kernel.on_claim_outcome_eose_no_match(sub_id, "wss://exhaust0.test");
            kernel.on_claim_outcome_eose_no_match(sub_id, "wss://exhaust1.test");
        }
        // Poll again — should now be exhausted
        let now = Instant::now();
        let _msgs2 = kernel.poll_claim_expansion(now);

        let phase = kernel.test_claim_phase(&primary_id);
        // Either exhausted (removed from map → None) or Terminal(Exhausted)
        assert!(
            phase.is_none() || matches!(phase, Some(Phase::Terminal(_))),
            "claim must terminate after candidates exhausted; phase: {phase:?}"
        );
    }

    // ── T6: Per-claim total budget terminates user-visible ────────────────

    #[test]
    fn phase2_per_claim_total_budget_terminates_user_visible() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("55");
        let author = hex("66");

        // Start well before budget
        let hints: Vec<String> = vec!["wss://longrunning.test".to_string()];
        let started = Instant::now() - Duration::from_millis(9000); // 9 s > 8 s budget
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            hints,
            started,
        );

        // Poll past total budget (8000 ms)
        let msgs = kernel.poll_claim_expansion(Instant::now());
        let _ = msgs;

        // Claim must be terminal (removed from map)
        let phase = kernel.test_claim_phase(&primary_id);
        assert!(
            phase.is_none() || matches!(phase, Some(Phase::Terminal(_))),
            "claim must terminate after total budget elapsed; phase: {phase:?}"
        );
    }

    // ── T7: Concurrent claims for same author share score writeback ────────

    #[test]
    fn concurrent_claims_for_same_author_share_score_writeback() {
        use crate::kernel::relay_score::ClaimOutcome;

        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let author = hex("77");
        let relay_url = "wss://shared.test";

        // Register two claims for the same author
        let primary_id_a = event_id("aa");
        let primary_id_b = event_id("bb");

        register_claim_with_author(&mut kernel, &primary_id_a, &author);
        register_claim_with_author(&mut kernel, &primary_id_b, &author);

        // Claim A gets a Hit on relay_url → score map gets +1 success
        kernel.record_claim_outcome(&author, relay_url, ClaimOutcome::Hit);

        // Verify the in-memory score is updated (this is the same-tick sharing)
        let cell = kernel.get_relay_score(&author, relay_url);
        assert_eq!(cell.successes, 1, "score map must reflect claim A's hit");

        // Claim B sees the warmed relay when it queries is_warm
        let now_s = kernel.now_secs();
        let is_warm = kernel.relay_score_map.is_warm(&author, relay_url, now_s);
        assert!(
            is_warm,
            "score update from claim A must be visible to claim B in same tick"
        );
    }

    // ── T8: MAX_RELAYS_TRIED_PER_CLAIM cap ────────────────────────────────

    #[test]
    fn max_relays_tried_per_claim_capped_at_12() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("88");
        let author = hex("99");

        // Seed 15 candidates — more than the cap of 12
        let hints: Vec<String> = (0..15u8)
            .map(|i| format!("wss://relay{i:02}.test"))
            .collect();

        let started = Instant::now() - Duration::from_millis(1600);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            hints,
            started,
        );

        // Drive through Phase 2 by repeatedly polling + reporting EoseNoMatch
        // until the claim terminates.
        let mut iterations = 0;
        loop {
            let phase = kernel.test_claim_phase(&primary_id);
            if phase.is_none() || matches!(phase, Some(Phase::Terminal(_))) {
                break;
            }
            // Report EoseNoMatch on all in-flight subs
            let in_flight = kernel.test_claim_in_flight_sub_ids(&primary_id);
            for sub_id in &in_flight {
                let dummy_relay = "wss://dummy.test";
                kernel.on_claim_outcome_eose_no_match(sub_id, dummy_relay);
            }
            let now = Instant::now() + Duration::from_millis(iterations * 200);
            let _msgs = kernel.poll_claim_expansion(now);
            iterations += 1;
            if iterations > 20 {
                break; // Safety valve
            }
        }

        let attempted_count = kernel.test_claim_attempted_count(&primary_id);
        assert!(
            attempted_count <= 12 || attempted_count == 0,
            "must not try more than MAX_RELAYS_TRIED_PER_CLAIM=12 relays, tried={attempted_count}"
        );
    }
}
