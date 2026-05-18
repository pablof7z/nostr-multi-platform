//! Exit-gate test 3 — Forward secrecy proof.
//!
//! Covers: remove a member, UpdateKeys (self_update+commit), assert the removed
//! member's epoch secrets CANNOT decrypt subsequent messages.
//!
//! Exit gate spec (marmot-mls.md §"Exit gate (product)"):
//!   "remove a member, send UpdateKeys, verify the removed member's epoch
//!    secrets cannot decrypt subsequent messages (MDK's process_message returns
//!    an error on the old credential)."
//!
//! ## What this proves
//!
//! After Alice removes Carol and commits a self_update (advancing the epoch),
//! Carol's MarmotService still holds the pre-removal MLS state. When Carol
//! calls `process_message` on a message encrypted at the new epoch, MDK must
//! return either:
//! - `Err(_)` — most common; Carol's leaf key can no longer decrypt the
//!   epoch's exporter secret, so the outer MIP-03 layer fails, OR
//! - `Ok(MessageProcessingResult::Unprocessable { .. })` — MDK stores it as
//!   unprocessable (e.g. out-of-epoch).
//!
//! Either outcome satisfies the forward-secrecy guarantee: Carol cannot read
//! the plaintext of post-removal messages.
//!
//! ## Three-member setup
//!
//! Alice (admin), Bob, Carol. Alice removes Carol. Alice self_updates (epoch
//! advance). Bob processes Alice's commits (sync). Alice sends a message. Carol
//! tries to decrypt it — must fail.

#[path = "marmot_harness.rs"]
mod harness;

use mdk_core::prelude::{GroupId, MessageProcessingResult, NostrGroupConfigData};
use nostr::{EventBuilder, Keys, Kind};

/// Add Carol to an existing Alice+Bob group. Returns the new group_id
/// (unchanged) and Carol's service.
fn add_carol_to_group(
    alice: &nmp_marmot::service::MarmotService,
    alice_keys: &Keys,
    bob: &nmp_marmot::service::MarmotService,
    carol: &nmp_marmot::service::MarmotService,
    carol_keys: &Keys,
    group_id: &GroupId,
) {
    // Carol publishes her key package.
    let carol_kp = carol
        .publish_key_package(harness::test_relays())
        .expect("carol publish kp");
    alice
        .validate_peer_key_package(&carol_kp.event_30443)
        .expect("alice validates carol kp");

    // Alice adds Carol.
    let add_pending = alice
        .add_members(group_id, &[carol_kp.event_30443.clone()])
        .expect("alice add_members carol");
    assert_eq!(add_pending.welcome_rumors.len(), 1, "one welcome for Carol");
    let carol_rumor = add_pending.welcome_rumors[0].clone();

    // Gift-wrap the welcome.
    let gift = alice
        .wrap_welcome(&carol_keys.public_key(), carol_rumor, None)
        .expect("alice gift-wraps carol welcome");

    // Commit the add.
    let add_event = add_pending.evolution_event.clone();
    add_pending.commit().expect("alice merges add commit");

    // Bob processes the add commit.
    match bob.process_message(&add_event).expect("bob processes add commit") {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit from add_members, got {other:?}"),
    }

    // Carol unwraps + accepts.
    let (carol_welcome, _sender) = carol
        .unwrap_and_process_welcome(&gift)
        .expect("carol unwraps welcome");
    carol
        .accept_welcome(&carol_welcome)
        .expect("carol accepts welcome");

    // Carol post-join self_update (MIP-02). Bob and Alice both process it.
    let carol_su = carol.self_update(group_id).expect("carol post-join self_update");
    let carol_su_event = carol_su.evolution_event.clone();
    carol_su.commit().expect("carol merges su");

    match alice.process_message(&carol_su_event).expect("alice processes carol su") {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("alice: expected Commit from carol su, got {other:?}"),
    }
    match bob.process_message(&carol_su_event).expect("bob processes carol su") {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("bob: expected Commit from carol su, got {other:?}"),
    }
}

#[test]
fn forward_secrecy_removed_member_cannot_decrypt() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let carol_keys = Keys::generate();

    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();
    let carol_dir = harness::TestDir::new();

    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());
    let carol = harness::service_at(&carol_dir.db_path("carol"), carol_keys.clone());

    // ── Establish Alice + Bob group ───────────────────────────────────────────
    let group_id = harness::setup_two_member_group(
        &alice, &alice_keys,
        &bob, &bob_keys,
        "forward-secrecy",
    );

    // ── Add Carol ─────────────────────────────────────────────────────────────
    add_carol_to_group(&alice, &alice_keys, &bob, &carol, &carol_keys, &group_id);

    // Verify Carol is in the member set before removal.
    let before_removal = alice.get_members(&group_id).expect("members before removal");
    assert!(
        before_removal.contains(&carol_keys.public_key()),
        "Carol must be in group before removal"
    );
    assert_eq!(before_removal.len(), 3, "3 members before removal");

    // ── Alice removes Carol ───────────────────────────────────────────────────
    let remove_pending = alice
        .remove_members(&group_id, &[carol_keys.public_key()])
        .expect("alice remove_members carol");
    let remove_event = remove_pending.evolution_event.clone();
    remove_pending.commit().expect("alice merges remove commit");

    // Bob processes the removal commit so both surviving members sync.
    match bob
        .process_message(&remove_event)
        .expect("bob processes remove commit")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit from remove, got {other:?}"),
    }

    // ── Alice (admin) UpdateKeys after removal (epoch advance) ────────────────
    let su_pending = alice.self_update(&group_id).expect("alice self_update");
    let su_event = su_pending.evolution_event.clone();
    su_pending.commit().expect("alice merges self_update");

    // Bob processes Alice's self_update commit to stay in sync.
    match bob
        .process_message(&su_event)
        .expect("bob processes alice self_update")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit from self_update, got {other:?}"),
    }

    // ── Alice sends a message encrypted at the new (post-removal) epoch ───────
    let plaintext = "carol cannot read this";
    let rumor = EventBuilder::new(Kind::TextNote, plaintext)
        .build(alice_keys.public_key());
    let msg_event = alice
        .create_message(&group_id, rumor)
        .expect("alice create_message post-removal");

    // ── Bob (still in group) CAN decrypt — sanity check ───────────────────────
    match bob
        .process_message(&msg_event)
        .expect("bob process_message")
    {
        MessageProcessingResult::ApplicationMessage(m) => {
            assert_eq!(m.content, plaintext, "Bob must decrypt correctly");
        }
        other => panic!("Bob: expected ApplicationMessage, got {other:?}"),
    }

    // ── Carol (removed) CANNOT decrypt — the forward-secrecy assertion ────────
    //
    // Carol's MLS state is frozen at the pre-removal epoch. When she tries to
    // process a message encrypted with post-removal epoch secrets, MDK must
    // return either Err (cannot decrypt outer MIP-03 layer) or
    // Ok(Unprocessable) (out-of-epoch / unknown group after removal).
    //
    // Both outcomes prove forward secrecy: Carol cannot obtain the plaintext.
    let carol_result = carol.process_message(&msg_event);
    let cannot_decrypt = match carol_result {
        Err(_) => true,
        Ok(MessageProcessingResult::Unprocessable { .. }) => true,
        Ok(MessageProcessingResult::ApplicationMessage(ref m)) => {
            // This would be a failure: Carol should NOT get the plaintext.
            panic!(
                "FORWARD SECRECY FAILURE: removed Carol decrypted message: {:?}",
                m.content
            );
        }
        Ok(other) => {
            // Any non-ApplicationMessage result (Commit, Proposal, etc.) is
            // also acceptable — Carol cannot read the plaintext.
            let _ = other;
            true
        }
    };
    assert!(
        cannot_decrypt,
        "removed Carol must not be able to decrypt post-removal messages"
    );

    // ── Members after removal ─────────────────────────────────────────────────
    let after_removal = alice.get_members(&group_id).expect("members after removal");
    assert!(!after_removal.contains(&carol_keys.public_key()), "Carol not in group");
    assert_eq!(after_removal.len(), 2, "2 members after removal");
}
