//! Bonus regression coverage: the no-signer-bound challenge path, and the
//! actor-flow integration that claim REQs are partitioned through the AUTH
//! gate at the single `send_all_outbound` choke point.

use super::*;

// ───────────────────────────────────────────────────────────────────────────
// Bonus regression: AUTH with no signer bound stays in ChallengeReceived
// (the iOS-not-yet-authenticated case). Documents the no-signer path so
// future agents don't accidentally make it a panic.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn nip42_kernel_auth_without_signer_holds_in_challenge_received() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let outbound = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch1"),
    );
    assert!(outbound.is_empty(), "no signer = no wire frame emitted");
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::ChallengeReceived
    );
    assert!(kernel.relay_auth_paused(RelayRole::Content));
}

// ───────────────────────────────────────────────────────────────────────────
// Bonus regression: actor-flow integration — claim REQs are partitioned
// at the single `send_all_outbound` choke point. This test mirrors what the
// actor does for ActorCommand::ClaimProfile: it calls `kernel.resolve_ref()`
// (which emits a kind:0 REQ to the Indexer) and feeds the output through
// `partition_auth_paused` (the routine `send_all_outbound` calls). Without
// the relay_mgmt.rs choke-point change, this test would fail — the claim REQs
// would bypass the AUTH gate.
//
// V-112 (ADR-0076): original test used `kernel.open_author()` (deleted).
// ADR-0070 Lane H: migrated from `kernel.claim_profile()` to `kernel.resolve_ref()`.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn nip42_kernel_claim_reqs_routed_through_auth_gate() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let (signer, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // Drive Indexer into ChallengeReceived → Authenticating.
    let _ = kernel.handle_text(
        RelayRole::Indexer,
        RelayRole::Indexer.url(),
        &auth_frame("ch1"),
    );
    assert!(kernel.relay_auth_paused(RelayRole::Indexer));

    // Claim a profile (M2: registers a kind:0 interest; the planner emits the
    // REQ on drain). The Indexer-bound kind:0 REQ must NOT reach the wire while
    // the Indexer relay is AUTH-paused.
    let _ = kernel.resolve_ref(
        RefNamespace::Profile,
        "1234567812345678123456781234567812345678123456781234567812345678".to_string(),
        "auth-gate-test".to_string(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk.into(),
        false,
        Vec::new(),
    );
    let drained = kernel.drain_lifecycle_outbound();
    let post_partition = kernel.partition_auth_paused(drained);

    // No Indexer-targeted REQ makes it through while AUTH-paused.
    assert!(
        !post_partition
            .iter()
            .any(|m| m.role == RelayRole::Indexer && m.text.starts_with("[\"REQ\"")),
        "Indexer REQs must be diverted while AUTH-paused: {post_partition:?}"
    );
}
