//! Exit-gate test 1 — Key package lifecycle end-to-end.
//!
//! Covers: publish key package, peer fetches and validates it, creator creates
//! group, sends Welcome, peer joins.
//!
//! Exit gate spec (marmot-mls.md §"Exit gate (product)"):
//!   "publish key package, fetch peer's key package from relay, create group,
//!    send Welcome, peer joins group."
//!
//! Relay I/O is excluded (in-memory compute only); we prove the cryptographic
//! plumbing end-to-end without a real relay.

#[path = "marmot_harness.rs"]
mod harness;

use nostr::{Keys, Kind};

#[test]
fn key_package_lifecycle_publish_validate_create_group_join() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();

    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());

    // ── Step 1: Bob publishes a KeyPackage (dual kind:30443 + legacy 443) ────
    let bob_kp = bob
        .publish_key_package(harness::test_relays())
        .expect("bob publish_key_package");

    // Contract: dual-publish events are both present and have correct kinds.
    assert_eq!(bob_kp.event_30443.kind, Kind::Custom(30443));
    assert_eq!(bob_kp.event_443.kind, Kind::Custom(443));
    // Contract: d_tag is non-empty (required for relay-side replacement on rotation).
    assert!(!bob_kp.d_tag.is_empty(), "d_tag must be non-empty");
    // Contract: hash_ref is non-empty (lifecycle tracking).
    assert!(!bob_kp.hash_ref.is_empty(), "hash_ref must be non-empty");

    // ── Step 2: Alice "fetches" Bob's key package (validates it parses) ──────
    // In production this is a relay fetch; here we pass the event directly to
    // simulate the relay-fetch result.
    alice
        .validate_peer_key_package(&bob_kp.event_30443)
        .expect("alice validates Bob's key package");

    // ── Step 3: Alice creates a group inviting Bob ────────────────────────────
    let (group, pending) = alice
        .create_group(
            vec![bob_kp.event_30443.clone()],
            harness::group_config("lifecycle-test", &alice_keys),
        )
        .expect("alice create_group");

    let group_id = group.mls_group_id.clone();

    // Contract: exactly one welcome rumor for Bob.
    assert_eq!(pending.welcome_rumors.len(), 1, "one welcome rumor for Bob");
    let bob_rumor = pending.welcome_rumors[0].clone();
    assert_eq!(bob_rumor.kind, Kind::MlsWelcome, "rumor must be kind:444");

    // ── Step 4: Alice NIP-59 gift-wraps the Welcome and sends it to Bob ──────
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), bob_rumor)
        .expect("alice wrap_welcome");
    assert_eq!(
        gift.kind,
        Kind::GiftWrap,
        "outer gift-wrap must be kind:1059"
    );

    // Commit the create (publish-success path).
    pending.commit().expect("alice merge create commit");

    // ── Step 5: Bob unwraps + processes + accepts the Welcome ─────────────────
    let (bob_welcome, sender) = bob
        .unwrap_and_process_welcome(&gift)
        .expect("bob unwrap_and_process_welcome");
    assert_eq!(
        sender,
        alice_keys.public_key(),
        "gift-wrap seal sender must be Alice"
    );
    bob.accept_welcome(&bob_welcome)
        .expect("bob accept_welcome");

    // ── Step 6: Post-join self_update (MIP-02 mandatory) ─────────────────────
    harness::post_join_self_update(&bob, &alice, &group_id);

    // ── Step 7: Verify membership on both sides ───────────────────────────────
    let alice_members = alice.get_members(&group_id).expect("alice get_members");
    assert_eq!(alice_members.len(), 2, "Alice sees 2 members");
    assert!(
        alice_members.contains(&alice_keys.public_key()),
        "Alice is a member"
    );
    assert!(
        alice_members.contains(&bob_keys.public_key()),
        "Bob is a member"
    );

    let bob_members = bob.get_members(&group_id).expect("bob get_members");
    assert_eq!(bob_members.len(), 2, "Bob sees 2 members");
    assert!(
        bob_members.contains(&alice_keys.public_key()),
        "Bob sees Alice"
    );
    assert!(
        bob_members.contains(&bob_keys.public_key()),
        "Bob sees himself"
    );

    // ── Step 8: get_group returns the group on both sides ─────────────────────
    let alice_group = alice
        .get_group(&group_id)
        .expect("alice get_group ok")
        .expect("alice group exists");
    assert_eq!(alice_group.mls_group_id, group_id);

    let bob_group = bob
        .get_group(&group_id)
        .expect("bob get_group ok")
        .expect("bob group exists");
    assert_eq!(bob_group.mls_group_id, group_id);
}

/// Verify the legacy kind:443 event from publish_key_package also validates.
#[test]
fn key_package_legacy_kind_443_validates() {
    let bob_keys = Keys::generate();
    let alice_keys = Keys::generate();

    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();

    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());

    let bob_kp = bob
        .publish_key_package(harness::test_relays())
        .expect("bob publish kp");

    // Legacy kind:443 must also parse successfully.
    alice
        .validate_peer_key_package(&bob_kp.event_443)
        .expect("alice validates legacy kind:443");
}
