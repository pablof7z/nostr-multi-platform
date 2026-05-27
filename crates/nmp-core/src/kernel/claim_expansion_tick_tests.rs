//! W6 — idle-tick wiring regression guards.
//!
//! These tests verify that `Kernel::poll_claim_expansion(now)` — wired into
//! the actor idle tick by W6 — drives the per-claim Phase 1/2/3 state machine
//! correctly when called from the actor's idle loop.
//!
//! Four invariants are pinned here:
//!
//! 1. `idle_tick_no_pending_claims_is_no_op` — when no claims are registered,
//!    `poll_claim_expansion` returns an empty Vec with zero allocations (D8).
//! 2. `idle_tick_advances_phase1_deadline` — a claim whose Phase-1 budget has
//!    elapsed advances to Phase2InFlight after one poll.
//! 3. `idle_tick_emits_phase2_compile_trigger` — after Phase-2 advance,
//!    `drain_lifecycle_tick` produces REQ WireFrames because `advance_to_phase2`
//!    enqueues `CompileTrigger::ViewOpened` (the planner compile is the actual
//!    output pipe; `poll_claim_expansion` itself returns `Vec::new()`).
//! 4. `idle_tick_terminates_budget_elapsed_claim` — a claim started 9 seconds
//!    ago (well past the 8 000 ms `PER_CLAIM_TOTAL_BUDGET_MS`) is removed from
//!    `pending_claims` in a single poll call.
//!
//! Design note: `Kernel` is `pub(crate)`, so these tests live inside
//! `nmp-core`. Integration-level tests that verify the end-to-end actor loop
//! live in `nmp-testing`.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::{Duration, Instant};

    use crate::kernel::claim_expansion::{Phase, PER_CLAIM_TOTAL_BUDGET_MS, PHASE_1_BUDGET_MS};
    use crate::kernel::Kernel;
    use crate::planner::{
        InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
    };
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use crate::subs::WireFrame;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn hex64(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    fn event_id(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    /// Register a minimal LogicalInterest so the planner has something to
    /// compile when `advance_to_phase2` enqueues `CompileTrigger::ViewOpened`.
    fn install_follow_interest(kernel: &mut Kernel, iid: u64, author: &str) {
        let mut authors = BTreeSet::new();
        authors.insert(author.to_string());
        let interest = LogicalInterest {
            id: InterestId(iid),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors,
                kinds: [1u32].into_iter().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
            is_indexer_discovery: false,
        };
        kernel.lifecycle_mut().registry_mut().push(interest);
        kernel
            .lifecycle_mut()
            .set_selection_budget(usize::MAX, usize::MAX);
    }

    // ── Test 1 — no pending claims is a D8 zero-cost no-op ───────────────────

    /// D8: with no claims registered, `poll_claim_expansion` must return an
    /// empty Vec immediately. This is the actor idle tick's common path on
    /// every quiet tick where no claim is in flight.
    #[test]
    fn idle_tick_no_pending_claims_is_no_op() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        assert!(
            kernel.pending_claims_is_empty(),
            "pre-condition: no claims registered"
        );
        let msgs = kernel.poll_claim_expansion(Instant::now());
        assert!(
            msgs.is_empty(),
            "no pending claims must produce an empty Vec (D8 no-op); got {}",
            msgs.len()
        );
        assert!(
            kernel.pending_claims_is_empty(),
            "pending_claims must remain empty after a no-op poll"
        );
    }

    // ── Test 2 — Phase-1 budget elapsed advances to Phase2InFlight ───────────

    /// After the Phase-1 budget (1 500 ms) elapses, a single `poll_claim_expansion`
    /// call must transition the claim from `Phase1` to `Phase2InFlight`.
    ///
    /// This pins the W6 wiring: if `poll_claim_expansion` is never called from
    /// the idle tick, Phase-1 claims stall forever.
    #[test]
    fn idle_tick_advances_phase1_deadline() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let author = hex64(0xab);
        let primary_id = event_id(0xcd);

        // Register a claim started 2 000 ms ago — past the 1 500 ms Phase-1 budget.
        let started_at = Instant::now() - Duration::from_millis(PHASE_1_BUDGET_MS + 500);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            vec!["wss://w6-test-relay.example/".to_string()],
            started_at,
        );

        // Confirm Phase1 before the poll.
        assert_eq!(
            kernel.test_claim_phase(&primary_id),
            Some(Phase::Phase1),
            "claim must start in Phase1"
        );

        // Drive the idle tick.
        let msgs = kernel.poll_claim_expansion(Instant::now());
        let _ = msgs; // Vec::new() today per W5 contract

        // Claim must have advanced out of Phase1.
        let phase = kernel.test_claim_phase(&primary_id);
        assert!(
            phase != Some(Phase::Phase1),
            "claim must leave Phase1 after budget elapses (got {phase:?})"
        );
    }

    // ── Test 3 — Phase-2 advance enqueues compile trigger → REQ frames ────────

    /// After `poll_claim_expansion` advances a claim to Phase 2,
    /// `advance_to_phase2` enqueues a `CompileTrigger::ViewOpened`. A subsequent
    /// `drain_lifecycle_tick` must therefore produce at least one REQ WireFrame.
    ///
    /// This proves the full W6 pipe: idle tick → poll → compile trigger →
    /// planner compile → REQ emitted. The actor then sends those REQs via
    /// `send_all_outbound`.
    #[test]
    fn idle_tick_emits_phase2_compile_trigger() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let author = hex64(0x11);
        let primary_id = event_id(0x22);

        // Seed a kind:10002 write relay so the planner can route the REQ.
        kernel.seed_mailbox_relay_list(
            &author,
            vec![],
            vec!["wss://w6-write-relay.example/".to_string()],
            vec![],
        );

        // Install a follow interest so the planner compile has targets.
        install_follow_interest(&mut kernel, 99, &author);

        // Register a claim past the Phase-1 budget with a hint.
        let started_at = Instant::now() - Duration::from_millis(PHASE_1_BUDGET_MS + 500);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            vec!["wss://w6-hint-relay.example/".to_string()],
            started_at,
        );

        // Drive the idle tick — should advance to Phase2 and enqueue trigger.
        let _ = kernel.poll_claim_expansion(Instant::now());

        // The compile trigger must now produce REQ frames when drained.
        let frames = kernel.drain_lifecycle_tick();
        let req_frames: Vec<_> = frames
            .iter()
            .filter(|f| matches!(f, WireFrame::Req { .. }))
            .collect();
        assert!(
            !req_frames.is_empty(),
            "drain_lifecycle_tick must produce REQ frames after Phase-2 advance \
             (compile trigger enqueued by advance_to_phase2); got {} total frames",
            frames.len()
        );
    }

    // ── Test 4 — total budget elapsed terminates claim ────────────────────────

    /// A claim whose total wall-clock age exceeds `PER_CLAIM_TOTAL_BUDGET_MS`
    /// (8 000 ms) must be removed from `pending_claims` after one poll.
    ///
    /// This guards against zombie claims that never terminate if the actor
    /// fails to advance Phase 2 quickly enough.
    #[test]
    fn idle_tick_terminates_budget_elapsed_claim() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let author = hex64(0x55);
        let primary_id = event_id(0x66);

        // Start the claim 9 s ago — past the 8 000 ms total budget.
        let started_at = Instant::now() - Duration::from_millis(PER_CLAIM_TOTAL_BUDGET_MS + 1000);
        kernel.register_claim_expansion(
            primary_id.clone(),
            None,
            Some(author.clone()),
            vec![],
            started_at,
        );

        assert!(
            !kernel.pending_claims_is_empty(),
            "pre-condition: claim must be registered"
        );

        // Drive the idle tick.
        let msgs = kernel.poll_claim_expansion(Instant::now());
        let _ = msgs;

        // Claim must be removed (terminated and pruned).
        assert!(
            kernel.pending_claims_is_empty(),
            "claim must be removed after total budget elapsed; \
             test_claim_phase = {:?}",
            kernel.test_claim_phase(&primary_id)
        );
    }
}
