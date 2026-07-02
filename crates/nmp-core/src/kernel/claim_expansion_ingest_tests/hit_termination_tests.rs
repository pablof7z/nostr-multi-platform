//! T-P1 / T-P3 — a wire EVENT (or a direct `on_claim_outcome_hit` sub_id
//! call) arriving through production ingest drives the W5 controller to
//! Terminal(Hit) and drains both `pending_claims` and `claim_sub_index` (B3
//! cleanup invariant).

use std::time::Instant;

use super::claim_expansion_ingest_support::{event_frame, setup_kernel_with_wired_claim};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_network::role::RelayRole;

// ── T-P1: EVENT through production ingest terminates claim ──────────────

/// Verify that a wire EVENT arriving through `handle_text` drives the W5
/// controller to Terminal(Hit) AND drains `claim_sub_index` to empty.
///
/// This is the core production wire-up test (META codex finding). Pre-fix,
/// `record_claim_expansion_hit` recorded the score but never called
/// `on_claim_outcome_hit`, so `pending_claims` was never cleared.
#[test]
fn claim_terminates_via_production_event_ingest() {
    use crate::kernel::test_support;

    let relay_url = "wss://claim-test.relay";
    test_support::clear_claim_expansion_subs();

    let (mut kernel, sub_id, event) = setup_kernel_with_wired_claim(relay_url);

    // Verify the claim is registered
    assert!(
        !kernel.pending_claims_is_empty(),
        "claim must be registered before EVENT arrives"
    );
    assert_eq!(
        kernel.test_claim_sub_index_len(),
        1,
        "claim_sub_index must have one entry after wire-frame registration"
    );

    // Deliver the matching EVENT through the production handle_text path.
    kernel.handle_text(RelayRole::Indexer, relay_url, &event_frame(&sub_id, &event));

    // The claim must be terminated and both maps must be empty.
    assert!(
        kernel.pending_claims_is_empty(),
        "pending_claims must be empty after Terminal(Hit)"
    );
    assert_eq!(
        kernel.test_claim_sub_index_len(),
        0,
        "claim_sub_index must be empty after Terminal(Hit) (B3 cleanup)"
    );

    test_support::clear_claim_expansion_subs();
}

// ── T-P3: claim_sub_index drains to zero after hit ──────────────────────

/// Verify that after Terminal(Hit), `claim_sub_index` is empty (B3 invariant).
/// Uses the test-support path for the claim_sub_index population so we can
/// assert the cleanup without depending on the planner's filter hash.
#[test]
fn claim_sub_index_drains_to_zero_after_hit() {
    use crate::planner::InterestId;
    use crate::subs::WireFrame;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let relay_url = "wss://index-drain.relay";
    let primary_id = "a".repeat(64);
    let author = "b".repeat(64);
    let sub_id = "sub-test-drain-01";

    // Register the claim
    kernel.register_claim_expansion(
        primary_id.clone(),
        None,
        Some(author.clone()),
        vec![relay_url.to_string()],
        Instant::now(),
    );

    // Inject a wire frame to populate claim_sub_index
    let frames = vec![WireFrame::Req {
        relay_url: relay_url.to_string(),
        sub_id: sub_id.to_string(),
        filter_json: r#"{"ids":["test"],"limit":1}"#.to_string(),
        interest_id: InterestId(0),
        lifecycle: crate::planner::InterestLifecycle::OneShot,
    }];
    kernel.register_wire_frames_for_test(&frames);

    assert_eq!(
        kernel.test_claim_sub_index_len(),
        1,
        "claim_sub_index must have one entry after wire-frame inject"
    );

    // Terminate via on_claim_outcome_hit (sub_id path)
    kernel.on_claim_outcome_hit(sub_id);

    assert_eq!(
        kernel.test_claim_sub_index_len(),
        0,
        "claim_sub_index must be 0 after Terminal(Hit) via sub_id (B3)"
    );
    assert!(
        kernel.pending_claims_is_empty(),
        "pending_claims must be empty after Terminal(Hit)"
    );
}
