//! Error paths: invalid operations surface errors — or, for a dropped
//! pending change, self-heal — instead of panicking.

use nostr::{EventBuilder, Kind};

use super::fixtures::{bootstrap_pair, new_actor};

/// `leave_group` against a group id that was never created must surface a
/// `MarmotError::Mdk` error, not panic.
#[test]
fn leave_nonexistent_group_errors() {
    let alice = new_actor();
    let bogus = mdk_core::prelude::GroupId::from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    // Map to a borrow-free Result first: an Ok holds a `PendingGroupChange`
    // that borrows `alice.service`, which would outlive the binding here.
    let outcome: std::result::Result<(), crate::service::MarmotError> =
        alice.service.leave_group(&bogus).map(|p| {
            drop(p);
        });
    match outcome {
        Err(crate::service::MarmotError::Mdk(_)) => {}
        Err(other) => panic!("expected Mdk error for unknown group, got {other:?}"),
        Ok(()) => panic!("leaving a non-existent group must not succeed"),
    }
}

/// `remove_members` for a pubkey that is not a member must error rather than
/// silently succeed or panic.
#[test]
fn remove_non_member_errors() {
    let alice = new_actor();
    let bob = new_actor();
    let stranger = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    let removed_non_member = alice
        .service
        .remove_members(&group_id, &[stranger.pubkey()])
        .map(drop)
        .is_ok();
    assert!(
        !removed_non_member,
        "removing a non-member must not succeed"
    );
    // The group is not wedged: a real op still works afterwards.
    let su = alice
        .service
        .self_update(&group_id)
        .expect("self_update still works after failed remove");
    su.commit().expect("merge self_update");
}

/// `validate_peer_key_package` against a wrong-kind event (a plain text note)
/// must reject it — it is a pre-flight sanity check for kind:30443.
#[test]
fn validate_rejects_non_key_package_event() {
    let alice = new_actor();
    let not_a_kp = EventBuilder::new(Kind::TextNote, "definitely not a key package")
        .sign_with_keys(&alice.keys)
        .expect("sign text note");
    assert!(
        alice.service.validate_peer_key_package(&not_a_kp).is_err(),
        "a kind:1 text note is not a valid KeyPackage event"
    );
}

/// `unwrap_and_process_welcome` against an event that is not a NIP-59
/// gift-wrap must surface an error (GiftWrap unwrap failure), not panic.
#[test]
fn unwrap_rejects_non_gift_wrap_event() {
    let alice = new_actor();
    let not_a_gift = EventBuilder::new(Kind::TextNote, "not a gift wrap")
        .sign_with_keys(&alice.keys)
        .expect("sign text note");
    match alice.service.unwrap_and_process_welcome(&not_a_gift) {
        Err(_) => {}
        Ok(_) => panic!("a kind:1 text note must not unwrap as a Welcome"),
    }
}

/// Dropping a `PendingGroupChange` without commit/clear must NOT wedge the
/// group — the `Drop` impl defensively clears the pending commit.
#[test]
fn dropped_pending_change_does_not_wedge_group() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    // Create a pending self_update and drop it WITHOUT commit/clear.
    {
        let pending = alice.service.self_update(&group_id).expect("self_update");
        drop(pending); // Drop impl must clear the pending commit.
    }
    // The group is not wedged: a fresh op succeeds.
    let su = alice
        .service
        .self_update(&group_id)
        .expect("self_update after dropped pending");
    su.commit().expect("merge after dropped pending");
}
