//! V-61: orphaned commit counter.

use super::fixtures::{bootstrap_pair, new_actor};

/// V-61: dropping a `PendingGroupChange` without commit/clear must increment
/// `orphaned_commit_count` so the host can observe the divergence.
///
/// The group must not be wedged (the existing behaviour), AND the counter
/// must record the event so a downstream snapshot can surface it.
#[test]
fn dropped_pending_change_increments_orphaned_commit_count() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    assert_eq!(
        alice.service.orphaned_commit_count(),
        0,
        "counter starts at zero"
    );

    // Drop without commit/clear.
    drop(alice.service.self_update(&group_id).expect("self_update"));

    assert_eq!(
        alice.service.orphaned_commit_count(),
        1,
        "one drop must increment the counter exactly once"
    );

    // A second unresolved drop accumulates.
    drop(alice.service.self_update(&group_id).expect("self_update 2"));
    assert_eq!(
        alice.service.orphaned_commit_count(),
        2,
        "second unresolved drop must accumulate"
    );

    // A resolved commit does NOT increment.
    alice
        .service
        .self_update(&group_id)
        .expect("self_update 3")
        .commit()
        .expect("commit");
    assert_eq!(
        alice.service.orphaned_commit_count(),
        2,
        "resolved commit must not increment the counter"
    );

    // A resolved clear does NOT increment.
    alice
        .service
        .self_update(&group_id)
        .expect("self_update 4")
        .clear()
        .expect("clear");
    assert_eq!(
        alice.service.orphaned_commit_count(),
        2,
        "resolved clear must not increment the counter"
    );
}

/// V-61: a SelfRemove (`leave_group`) dropped unresolved must NOT increment
/// the counter — SelfRemove never creates a pending commit on the local side.
#[test]
fn dropped_self_remove_does_not_increment_orphaned_count() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    assert_eq!(bob.service.orphaned_commit_count(), 0);
    // Bob is a non-admin member; leave_group is SelfRemove for non-admins.
    // A peer commits it — no local pending commit is created.
    let pending = bob.service.leave_group(&group_id).expect("leave_group");
    drop(pending);
    assert_eq!(
        bob.service.orphaned_commit_count(),
        0,
        "SelfRemove drop must not increment orphaned_commit_count"
    );
}
