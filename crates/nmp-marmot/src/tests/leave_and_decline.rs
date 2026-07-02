//! `leave_group`: SelfRemove proposal; `commit()` does NOT merge.
//! `decline_welcome`: invitee rejects; group is `Inactive`, not joined.

use mdk_core::prelude::MessageProcessingResult;

use super::fixtures::{bootstrap_pair, group_config, new_actor, test_relays};

/// `leave_group` is a SelfRemove proposal: a peer commits the epoch, so the
/// leaver's `PendingGroupChange::commit()` is a documented no-op. The admin
/// processes the leaver's proposal as a Proposal (not a Commit).
#[test]
fn leave_group_is_self_remove_and_commit_is_noop() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);
    assert_eq!(alice.service.get_members(&group_id).unwrap().len(), 2);

    // Bob leaves: SelfRemove proposal.
    let leave = bob.service.leave_group(&group_id).expect("bob leaves");
    let leave_event = leave.evolution_event.clone();
    // commit() on a SelfRemove handle must succeed WITHOUT merging an MLS
    // commit (no pending commit was created — a peer commits the epoch).
    leave.commit().expect("bob's SelfRemove commit is a no-op");

    // The admin processes Bob's leave proposal. A bare `leave_group` emits a
    // SelfRemove *Proposal* — a peer (admin) commits the epoch later. If MDK
    // ever starts auto-committing here this assertion catches the regression.
    match alice
        .service
        .process_message(&leave_event)
        .expect("alice processes bob's leave")
    {
        MessageProcessingResult::Proposal(_) => {}
        other => panic!("expected Proposal for a SelfRemove leave, got {other:?}"),
    }
}

/// An invitee who declines a Welcome does NOT join the group. MDK keeps the
/// group record (created `Pending` by `process_welcome`) but `decline_welcome`
/// flips it to `GroupState::Inactive` — verified against mdk-core 0.8.0
/// `welcomes.rs` (`process_welcome` → Pending; `decline_welcome` → Inactive).
/// The invariant a UI relies on: the declined group is never `Active`.
#[test]
fn decline_welcome_leaves_group_inactive_for_invitee() {
    use mdk_core::prelude::group_types::GroupState;

    let alice = new_actor();
    let bob = new_actor();

    let bob_kp = bob.service.publish_key_package(test_relays()).unwrap();
    let (group, pending) = alice
        .service
        .create_group(vec![bob_kp.event_30443], group_config(vec![alice.pubkey()]))
        .expect("alice creates group");
    let group_id = group.mls_group_id.clone();
    let rumor = pending.welcome_rumors[0].clone();
    let gift = alice
        .service
        .wrap_welcome(&bob.pubkey(), rumor)
        .expect("alice gift-wraps");
    pending.commit().unwrap();

    let (welcome, _) = bob
        .service
        .unwrap_and_process_welcome(&gift)
        .expect("bob processes welcome");
    bob.service
        .decline_welcome(&welcome)
        .expect("bob declines welcome");

    // Bob declined: the group record exists but is Inactive — never Active.
    let bob_group = bob
        .service
        .get_group(&group_id)
        .expect("get_group")
        .expect("declined group record is retained as Inactive");
    assert_eq!(
        bob_group.state,
        GroupState::Inactive,
        "a declined welcome must leave the group Inactive, never Active"
    );
}
