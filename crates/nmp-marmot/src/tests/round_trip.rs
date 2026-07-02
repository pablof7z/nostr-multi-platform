//! MDK + NIP-59 round-trip via the service API.

use mdk_core::prelude::{MessageProcessingResult, NostrGroupConfigData};
use nostr::{EventBuilder, Keys, Kind};

use super::fixtures::{in_memory_service, test_relays};

/// Full round-trip: Bob publishes a key package; Alice creates a group with
/// Bob's key package; Alice GIFT-WRAPS the Welcome (NIP-59) to Bob; Bob
/// unwraps + processes + accepts; Bob does the mandatory post-join
/// self-update; Alice sends a message; Bob decrypts it. Exercises the exact
/// public API a headless integration-test driver uses, including the real
/// `nmp_nip59` gift-wrap path.
#[test]
fn marmot_full_round_trip_create_giftwrap_join_message() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice = in_memory_service(alice_keys.clone());
    let bob = in_memory_service(bob_keys.clone());

    // Bob publishes a KeyPackage (kind:30443 only; legacy kind:443 retired).
    let bob_kp = bob
        .publish_key_package(test_relays())
        .expect("bob key package");
    assert_eq!(bob_kp.event_30443.kind, Kind::Custom(30443));
    assert!(!bob_kp.d_tag.is_empty());
    alice
        .validate_peer_key_package(&bob_kp.event_30443)
        .expect("alice validates bob kp");

    // Alice creates the group inviting Bob.
    let config = NostrGroupConfigData::new(
        "Marmot Test".to_string(),
        "round-trip".to_string(),
        None,
        None,
        None,
        test_relays(),
        vec![alice_keys.public_key()],
    );
    let (group, pending) = alice
        .create_group(vec![bob_kp.event_30443.clone()], config)
        .expect("alice creates group");
    let group_id = group.mls_group_id.clone();
    assert_eq!(pending.welcome_rumors.len(), 1, "one welcome for Bob");
    let bob_welcome_rumor = pending.welcome_rumors[0].clone();
    assert_eq!(bob_welcome_rumor.kind, Kind::MlsWelcome);

    // Alice NIP-59 gift-wraps the Welcome to Bob (real nmp_nip59 path).
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), bob_welcome_rumor)
        .expect("alice gift-wraps welcome");
    assert_eq!(gift.kind, Kind::GiftWrap);

    // Publish-success path: merge the create commit.
    pending.commit().expect("alice merges create commit");

    // Bob unwraps + processes + accepts the gift-wrapped Welcome.
    let (bob_welcome, sender) = bob
        .unwrap_and_process_welcome(&gift)
        .expect("bob unwraps + processes welcome");
    assert_eq!(sender, alice_keys.public_key(), "seal sender is Alice");
    bob.accept_welcome(&bob_welcome)
        .expect("bob accepts welcome");

    // Membership: Alice's view shows exactly Alice + Bob.
    let members = alice.get_members(&group_id).expect("members");
    assert_eq!(members.len(), 2);
    assert!(members.contains(&alice_keys.public_key()));
    assert!(members.contains(&bob_keys.public_key()));

    // Post-join self-update is mandatory per MIP-02. Bob rotates; Alice
    // processes Bob's commit so both converge on the new epoch.
    let bob_su = bob.self_update(&group_id).expect("bob self_update");
    let bob_commit = bob_su.evolution_event.clone();
    bob_su.commit().expect("bob merges self_update");
    match alice
        .process_message(&bob_commit)
        .expect("alice processes bob commit")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit, got {other:?}"),
    }

    // Alice sends an application message; Bob decrypts it.
    let rumor = EventBuilder::new(Kind::TextNote, "hello bob").build(alice_keys.public_key());
    let msg_event = alice
        .create_message(&group_id, rumor)
        .expect("alice creates message");
    assert_eq!(msg_event.kind, Kind::MlsGroupMessage);

    match bob
        .process_message(&msg_event)
        .expect("bob processes message")
    {
        MessageProcessingResult::ApplicationMessage(m) => {
            assert_eq!(m.content, "hello bob");
            assert_eq!(m.pubkey, alice_keys.public_key());
        }
        other => panic!("expected ApplicationMessage, got {other:?}"),
    }
}

/// The publish-FAILURE path: clear() unblocks future group ops
/// (mdk-api.md §7.7). After clear, a subsequent self_update must succeed.
#[test]
fn pending_change_clear_unblocks_group_ops() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let alice = in_memory_service(alice_keys.clone());
    let bob = in_memory_service(bob_keys.clone());

    let bob_kp = bob.publish_key_package(test_relays()).unwrap();
    let config = NostrGroupConfigData::new(
        "g".into(),
        "d".into(),
        None,
        None,
        None,
        test_relays(),
        vec![alice_keys.public_key()],
    );
    let (group, pending) = alice
        .create_group(vec![bob_kp.event_30443], config)
        .unwrap();
    let group_id = group.mls_group_id.clone();
    pending.commit().unwrap();

    // Simulate a publish failure on a self_update evolution_event: clear()
    // must reset the pending commit so a later op is not wedged.
    let su = alice.self_update(&group_id).expect("self_update");
    su.clear().expect("clear pending commit on publish failure");

    // A fresh self_update now succeeds (group not wedged).
    let su2 = alice
        .self_update(&group_id)
        .expect("self_update after clear");
    su2.commit().expect("merge after clear");
}
