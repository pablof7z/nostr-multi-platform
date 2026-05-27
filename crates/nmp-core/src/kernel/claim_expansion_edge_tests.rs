//! TDD edge-case tests for W5 — T9–T13.
//!
//! Split from `claim_expansion_tests.rs` to keep each file under the D-V12
//! 500-LOC ceiling. Tests in this file cover §8.7 preflight, empty-outbox
//! exhaustion, mid-Phase-2 release, relay_failed writeback, and duplicate
//! registration no-op.

#[cfg(test)]
mod edge_tests {
    use std::time::{Duration, Instant};

    use crate::kernel::claim_expansion::Phase;
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    fn hex(byte: &str) -> String {
        byte.repeat(32)
    }

    fn event_id(byte: &str) -> String {
        byte.repeat(32)
    }

    // ── T9: Phase-1 hit same tick as budget does not emit Phase 2 ─────────

    #[test]
    fn phase1_hit_same_tick_as_budget_does_not_emit_phase2() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("a1");
        let author = hex("b2");

        let hints: Vec<String> = vec!["wss://phase2candidate.test".to_string()];
        // Set started_at such that we're exactly at Phase-1 budget
        let started = Instant::now() - Duration::from_millis(1600);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            hints,
            started,
        );

        // Mark event as known BEFORE poll (simulating same-tick hit)
        kernel.test_mark_event_known(&primary_id);

        let msgs = kernel.poll_claim_expansion(Instant::now());
        assert!(
            msgs.is_empty(),
            "already-known event must not produce Phase-2 REQs; got {} msgs",
            msgs.len()
        );

        // Claim must be terminated
        let phase = kernel.test_claim_phase(&primary_id);
        assert!(
            phase.is_none() || matches!(phase, Some(Phase::Terminal(_))),
            "claim with known event must be terminal; got {phase:?}"
        );
    }

    // ── T10: Phase 2 with empty outbox terminates exhausted ───────────────

    #[test]
    fn phase2_with_empty_outbox_terminates_exhausted() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("c3");
        let author = hex("d4");

        // No hints, author has no known outbox — candidate queue will be empty
        let started = Instant::now() - Duration::from_millis(1600);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            vec![], // empty hints
            started,
        );

        let _msgs = kernel.poll_claim_expansion(Instant::now());

        // With empty candidates, claim should terminate immediately
        let phase = kernel.test_claim_phase(&primary_id);
        assert!(
            phase.is_none() || matches!(phase, Some(Phase::Terminal(_))),
            "empty outbox + empty hints must terminate as Exhausted; got {phase:?}"
        );
    }

    // ── T11: Release mid-Phase-2 continues score writeback ────────────────

    #[test]
    fn release_mid_phase2_continues_score_writeback() {
        use crate::kernel::relay_score::ClaimOutcome;

        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("e5");
        let author = hex("f6");
        let relay_url = "wss://midphase2.test";

        let hints = vec![relay_url.to_string()];
        let started = Instant::now() - Duration::from_millis(1600);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            hints,
            started,
        );

        // Advance to Phase 2
        let _msgs = kernel.poll_claim_expansion(Instant::now());

        // Release the claim (simulates user navigating away)
        kernel.release_claim_expansion(&primary_id);

        // Score writeback must still work independently (D4: score map is
        // separate from pending_claims lifecycle)
        kernel.record_claim_outcome(&author, relay_url, ClaimOutcome::Hit);
        let cell = kernel.get_relay_score(&author, relay_url);
        assert_eq!(
            cell.successes, 1,
            "score writeback must succeed after claim release"
        );
    }

    // ── T12: relay_failed records Failed outcome for each matching claim ───

    #[test]
    fn relay_failed_records_failed_outcome_for_each_claim_that_attempted_the_relay() {
        use crate::kernel::relay_score::ClaimOutcome;

        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let author = hex("a9");
        let relay_url = "wss://failing.relay.test";

        // Register two claims that have attempted this relay
        let primary_id_a = event_id("ba");
        let primary_id_b = event_id("cb");

        let hints = vec![relay_url.to_string()];
        let started = Instant::now() - Duration::from_millis(1600);

        kernel.register_claim_expansion(
            primary_id_a.clone(),
            None,
            Some(author.clone()),
            hints.clone(),
            started,
        );
        kernel.register_claim_expansion(
            primary_id_b.clone(),
            None,
            Some(author.clone()),
            hints.clone(),
            started,
        );

        // Advance to Phase 2 so the relay is in the attempted set
        let _msgs = kernel.poll_claim_expansion(Instant::now());

        // Mark relay as attempted in both claims (simulates Phase-2 REQ emission)
        kernel.test_mark_claim_attempted(&primary_id_a, relay_url);
        kernel.test_mark_claim_attempted(&primary_id_b, relay_url);

        let failures_before = kernel.get_relay_score(&author, relay_url).failures;

        // Simulate relay_failed
        kernel.relay_failed_claim_walk(relay_url);

        let cell_after = kernel.get_relay_score(&author, relay_url);
        assert!(
            cell_after.failures > failures_before,
            "relay_failed must record Failed outcome for each matching claim; failures: {failures_before} → {}",
            cell_after.failures
        );
    }

    // ── T13: register_claim_expansion dedup (same primary_id twice) ───────

    #[test]
    fn register_claim_expansion_duplicate_primary_id_is_noop() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let primary_id = event_id("dc");
        let author = hex("ed");

        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            vec![],
            Instant::now(),
        );
        let count_before = kernel.test_pending_claims_count();

        // Second registration with same primary_id must be a no-op (D6)
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            vec![],
            Instant::now(),
        );
        let count_after = kernel.test_pending_claims_count();

        assert_eq!(
            count_before, count_after,
            "duplicate registration must be a no-op (D6)"
        );
    }
}
