//! T168 RED — logout / remove / switch must reconcile M2 follow interests.
//!
//! `remove_account` / `switch_active` only call
//! `sync_kernel`; they never reconcile the M2 follow-feed. With
//! `active_account=None` (logout of the last account)
//! `register_follow_feed_for_active_account()` early-returns, leaving the
//! previous account's `follow_feed_interest_ids` + `timeline_authors` LIVE on
//! the wire — a privacy leak + stale feed. On `switch_active` the prior
//! account's follow interests stay registered alongside the new account's.
//!
//! These tests MUST FAIL before the reconcile is wired into the identity
//! command paths and MUST PASS after.

use super::*;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fresh() -> (IdentityRuntime, Kernel) {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Declare the host kinds {1, 6} the contact-feed subscription REQs for,
    // as `nmp_app_chirp_open_home_feed` does in production. Without this the
    // kernel's `follow_feed_kinds` is empty and follow-feed registration is a
    // no-op (D0: the substrate no longer hardcodes a kind set).
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    (
        IdentityRuntime::new(
            new_bunker_handshake_slot(),
            crate::actor::new_signer_state_slot(),
        ),
        kernel,
    )
}

/// Sign in account A, register A's kind:3 follow set (follows ALICE), and
/// drain so the M2 follow-feed sub is registered + live on the wire.
fn sign_in_a_with_followfeed(id: &mut IdentityRuntime, kernel: &mut Kernel) -> String {
    // Pre-`AddSigner` this was `sign_in_nsec(id, kernel, TEST_NSEC, false)`;
    // the unified reducer's `LocalNsec` branch with `make_active: true` is the
    // direct replacement (the old `sign_in_nsec` always activated).
    add_signer(
        id,
        kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let active_pk = id.active_pubkey().expect("active account after sign_in");
    kernel.seed_kind10002_for_test(ALICE, &["wss://alice-t168.relay/"]);
    kernel.inject_replaceable_event(
        "0000000000000000000000000000000000000000000000000000000000000001",
        &active_pk,
        2_000,
        3,
        vec![vec!["p".to_string(), ALICE.to_string()]],
        "wss://seed.relay/",
        2_000_000,
    );
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);
    let frames = kernel.drain_lifecycle_tick();
    let reqs: Vec<&WireFrame> = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .collect();
    assert!(
        !reqs.is_empty(),
        "precondition: account A must have a live follow-feed REQ after \
         kind:3 + drain (got frames: {frames:?})"
    );
    // Register the emitted frames so the kernel tracks the wire subs.
    kernel.register_wire_frames_for_test(&frames);
    active_pk
}

fn close_count(frames: &[WireFrame]) -> usize {
    frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Close { .. }))
        .count()
}

/// remove_account of the ONLY account (→ active becomes None) must drive a
/// follow-feed reconcile: A's follow interests withdrawn + a CLOSE diff
/// emitted for A's now-orphaned follow-feed sub.
///
/// Pre-fix: only `sync_kernel` runs; `register_follow_feed_for_active_account`
/// early-returns on `active_account=None`; A's interests stay registered →
/// no CLOSE, interest set non-empty → FAILS.
/// Post-fix: identity path drives the reconcile → CLOSE emitted, interest set
/// empty → PASSES.
#[test]
fn t168_remove_only_account_closes_followfeed_and_clears_interests() {
    let (mut id, mut kernel) = fresh();
    let _a = sign_in_a_with_followfeed(&mut id, &mut kernel);

    assert!(
        !kernel.follow_feed_interest_ids_for_test().is_empty(),
        "precondition: A's follow-feed interests must be registered"
    );

    let only = kernel.account_snapshot().0[0].id.clone();
    remove_account(&mut id, &mut kernel, &only);

    // After remove, drain must emit the CLOSE diff for A's follow-feed sub.
    let frames = kernel.drain_lifecycle_tick();

    assert!(
        kernel.follow_feed_interest_ids_for_test().is_empty(),
        "T168: removing the only account must withdraw A's follow-feed \
         interests; still registered: {:?}",
        kernel.follow_feed_interest_ids_for_test()
    );
    assert!(
        !kernel.timeline_authors_for_test().contains(ALICE),
        "T168: ALICE (A's follow) must be gone from timeline_authors after \
         logout — stale-feed/privacy leak"
    );
    assert!(
        close_count(&frames) >= 1,
        "T168: removing the only account must emit a CLOSE for A's \
         orphaned follow-feed sub; got frames: {frames:?}"
    );
}

/// switch_active to a DIFFERENT account must reconcile follow interests to the
/// NEW account: A's follow interests withdrawn (a CLOSE diff), the new
/// account's (empty here) follow set installed.
///
/// Pre-fix: switch only calls `sync_kernel`; A's `timeline_authors` + follow
/// interests stay live alongside the new account → FAILS.
/// Post-fix: switch drives the reconcile → A's interests gone → PASSES.
#[test]
fn t168_switch_active_reconciles_followfeed_to_new_account() {
    let (mut id, mut kernel) = fresh();
    let a = sign_in_a_with_followfeed(&mut id, &mut kernel);

    // Add a freshly-generated second account (no kind:3 → empty follow set).
    // `create_account` makes it active; switch back to A (the account whose
    // follow-feed is live) so the `switch_active` under test moves A → second.
    let profile = std::collections::HashMap::new();
    let relays: Vec<(String, String)> = vec![];
    create_account(&mut id, &mut kernel, false, &profile, &relays, false, true);
    let second_id = id.active_pubkey().expect("second account active");
    switch_active(&mut id, &mut kernel, &a, false);
    let _ = kernel.drain_lifecycle_tick();

    // The path under test: switch the active account away from A.
    switch_active(&mut id, &mut kernel, &second_id, false);
    let frames = kernel.drain_lifecycle_tick();

    // The second account was created with DEFAULT_FOLLOWS (2 follows) + self-interest.
    assert_eq!(
        kernel.follow_feed_interest_ids_for_test().len(),
        3,
        "T168: switching to the second account must withdraw A's follow-feed \
         interests and install the second account's default follows + self: {:?}",
        kernel.follow_feed_interest_ids_for_test()
    );
    assert!(
        !kernel.timeline_authors_for_test().contains(ALICE),
        "T168: ALICE (account A's follow) must NOT remain in timeline_authors \
         after switching to a different account — stale-feed leak"
    );
    assert!(
        kernel.timeline_authors_for_test().contains(&second_id),
        "T168: second account's own pubkey must be in timeline_authors \
         after switch"
    );
    assert!(
        close_count(&frames) >= 1,
        "T168: switch_active must emit a CLOSE for A's orphaned follow-feed \
         sub; got frames: {frames:?}"
    );
}
