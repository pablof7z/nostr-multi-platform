//! `add_members`: existing group grows; both peers converge.
//! `remove_members`: group shrinks; remaining peer converges.

use mdk_core::prelude::MessageProcessingResult;

use super::fixtures::{bootstrap_pair, new_actor, test_relays};

/// Add a third member to an existing two-member group. The admin's view AND
/// the existing member's view must both project the new member count of 3
/// once the kind:445 commit is processed.
#[test]
fn add_members_grows_group_and_both_views_converge() {
    let alice = new_actor();
    let bob = new_actor();
    let carol = new_actor();

    let group_id = bootstrap_pair(&alice, &bob);
    assert_eq!(
        alice.service.get_members(&group_id).unwrap().len(),
        2,
        "alice + bob before invite"
    );

    // Carol publishes a KeyPackage; Alice (admin) adds her.
    let carol_kp = carol
        .service
        .publish_key_package(test_relays())
        .expect("carol key package");
    let pending = alice
        .service
        .add_members(&group_id, std::slice::from_ref(&carol_kp.event_30443))
        .expect("alice adds carol");
    let add_commit = pending.evolution_event.clone();
    let carol_rumor = pending.welcome_rumors[0].clone();
    let carol_gift = alice
        .service
        .wrap_welcome(&carol.pubkey(), carol_rumor)
        .expect("alice gift-wraps carol welcome");
    pending.commit().expect("alice merges add commit");

    // Alice's projection now shows 3 members.
    let alice_members = alice.service.get_members(&group_id).unwrap();
    assert_eq!(alice_members.len(), 3, "alice sees 3 after add");
    assert!(alice_members.contains(&carol.pubkey()));

    // Bob (an existing member) processes the add commit and converges to 3.
    match bob
        .service
        .process_message(&add_commit)
        .expect("bob processes add commit")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit, got {other:?}"),
    }
    let bob_members = bob.service.get_members(&group_id).unwrap();
    assert_eq!(bob_members.len(), 3, "bob converges to 3 after add commit");
    assert!(bob_members.contains(&carol.pubkey()));

    // Carol joins via her Welcome and also sees the 3-member group.
    let (carol_welcome, _) = carol
        .service
        .unwrap_and_process_welcome(&carol_gift)
        .expect("carol processes welcome");
    carol
        .service
        .accept_welcome(&carol_welcome)
        .expect("carol accepts welcome");
    assert_eq!(
        carol.service.get_members(&group_id).unwrap().len(),
        3,
        "carol's joined view shows 3 members"
    );
}

/// Remove a member from a three-member group. The admin's projection AND a
/// remaining member's projection must both fall to 2.
#[test]
fn remove_members_shrinks_group_and_view_converges() {
    let alice = new_actor();
    let bob = new_actor();
    let carol = new_actor();

    let group_id = bootstrap_pair(&alice, &bob);

    // Grow to 3 (Carol).
    let carol_kp = carol.service.publish_key_package(test_relays()).unwrap();
    let add = alice
        .service
        .add_members(&group_id, &[carol_kp.event_30443])
        .unwrap();
    let add_commit = add.evolution_event.clone();
    add.commit().unwrap();
    bob.service.process_message(&add_commit).unwrap();
    assert_eq!(alice.service.get_members(&group_id).unwrap().len(), 3);

    // Alice removes Carol.
    let removal = alice
        .service
        .remove_members(&group_id, &[carol.pubkey()])
        .expect("alice removes carol");
    let remove_commit = removal.evolution_event.clone();
    removal.commit().expect("alice merges remove commit");

    let alice_members = alice.service.get_members(&group_id).unwrap();
    assert_eq!(alice_members.len(), 2, "alice sees 2 after remove");
    assert!(!alice_members.contains(&carol.pubkey()), "carol removed");

    // Bob processes the remove commit and converges to 2.
    match bob
        .service
        .process_message(&remove_commit)
        .expect("bob processes remove commit")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit, got {other:?}"),
    }
    let bob_members = bob.service.get_members(&group_id).unwrap();
    assert_eq!(bob_members.len(), 2, "bob converges to 2 after remove");
    assert!(!bob_members.contains(&carol.pubkey()));
}
