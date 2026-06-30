//! Unit tests for the DM inbox account-switch teardown.
//!
//! Verifies that [`super::DmInboxController::on_account_change`] clears the
//! projection when the active account changes, so the previous account's
//! decrypted DMs never leak into the new account's UI.

use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::actor::{ActorCommand, SignCommand};
use nmp_core::{ActorMail, CommandSender};
use nostr::{EventBuilder, JsonUtil, Keys, Kind, PublicKey, Tag, Timestamp};

use crate::DmInboxProjection;

use super::DmInboxController;

fn gift_wrapped_dm(sender: &Keys, receiver: &PublicKey, content: &str, ts: u64) -> String {
    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(vec![Tag::public_key(*receiver)])
        .custom_created_at(Timestamp::from(ts))
        .build(sender.public_key());
    nmp_nip59::gift_wrap_local(sender, receiver, &rumor, Timestamp::from(ts))
        .expect("gift wrap succeeds")
        .as_json()
}

fn feed_dm(
    proj: &DmInboxProjection,
    rx: &Receiver<ActorMail>,
    receiver_keys: &Keys,
    envelope: &str,
) {
    proj.ingest_gift_wrap(envelope, None);
    drive_decrypts(rx, receiver_keys);
}

fn drive_decrypts(rx: &Receiver<ActorMail>, keys: &Keys) {
    while let Ok(mail) = rx.try_recv() {
        let ActorMail::Command(ActorCommand::Sign(SignCommand::Nip44DecryptForAccount {
            peer_pubkey,
            ciphertext,
            continuation,
            ..
        })) = mail
        else {
            continue;
        };
        let outcome = PublicKey::from_hex(&peer_pubkey)
            .map_err(|e| e.to_string())
            .and_then(|peer| {
                nostr::nips::nip44::decrypt(keys.secret_key(), &peer, &ciphertext)
                    .map_err(|e| e.to_string())
            });
        continuation.call(outcome);
    }
}

fn controller_for(
    pubkey: &PublicKey,
) -> (
    DmInboxController,
    Arc<Mutex<Option<String>>>,
    Receiver<ActorMail>,
) {
    let active: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(Some(pubkey.to_hex())));
    let (tx, rx) = channel::<ActorMail>();
    let controller = DmInboxController::new(Arc::clone(&active), CommandSender::new(tx));
    (controller, active, rx)
}

#[test]
fn account_switch_clears_previous_accounts_messages() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let carol = Keys::generate();

    let (controller, active, rx) = controller_for(&alice.public_key());
    let proj = controller.inbox_slot();

    let envelope = gift_wrapped_dm(&carol, &alice.public_key(), "secret for alice", 100);
    feed_dm(&proj, &rx, &alice, &envelope);
    assert_eq!(
        proj.snapshot().conversations.len(),
        1,
        "Alice should see the DM before account switch"
    );

    *active.lock().unwrap() = Some(bob.public_key().to_hex());
    let changed = controller.on_account_change();
    assert!(
        changed,
        "on_account_change must return true when the pubkey changed"
    );

    assert!(
        proj.snapshot().conversations.is_empty(),
        "after account switch, previous account DMs must not appear"
    );
}

#[test]
fn new_account_receives_its_own_dms_after_switch() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let carol = Keys::generate();

    let (controller, active, rx) = controller_for(&alice.public_key());
    let proj = controller.inbox_slot();

    feed_dm(
        &proj,
        &rx,
        &alice,
        &gift_wrapped_dm(&carol, &alice.public_key(), "for alice", 200),
    );

    *active.lock().unwrap() = Some(bob.public_key().to_hex());
    controller.on_account_change();

    feed_dm(
        &proj,
        &rx,
        &bob,
        &gift_wrapped_dm(&carol, &bob.public_key(), "for bob", 300),
    );

    let snap = proj.snapshot();
    assert_eq!(snap.conversations.len(), 1, "Bob should see his DM");
    assert_eq!(
        snap.conversations[0].messages[0].content, "for bob",
        "the message must be Bob's, not Alice's"
    );
}

#[test]
fn sign_out_clears_inbox() {
    let alice = Keys::generate();
    let carol = Keys::generate();

    let (controller, active, rx) = controller_for(&alice.public_key());
    let proj = controller.inbox_slot();

    feed_dm(
        &proj,
        &rx,
        &alice,
        &gift_wrapped_dm(&carol, &alice.public_key(), "private", 100),
    );
    assert_eq!(
        proj.snapshot().conversations.len(),
        1,
        "sanity: DM ingested"
    );

    *active.lock().unwrap() = None;
    let changed = controller.on_account_change();
    assert!(changed, "sign-out must be detected as a change");

    assert!(
        proj.snapshot().conversations.is_empty(),
        "sign-out must clear the inbox"
    );
}

#[test]
fn no_change_does_not_clear_projection() {
    let alice = Keys::generate();
    let carol = Keys::generate();

    let (controller, _active, rx) = controller_for(&alice.public_key());
    let proj = controller.inbox_slot();

    feed_dm(
        &proj,
        &rx,
        &alice,
        &gift_wrapped_dm(&carol, &alice.public_key(), "for alice", 100),
    );
    assert_eq!(
        proj.snapshot().conversations.len(),
        1,
        "sanity: DM ingested"
    );

    let changed = controller.on_account_change();
    assert!(
        !changed,
        "on_account_change must return false when the pubkey is unchanged"
    );

    assert_eq!(
        proj.snapshot().conversations.len(),
        1,
        "DMs must survive a no-op on_account_change"
    );
}

#[test]
fn multiple_consecutive_switches_each_clear() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let carol = Keys::generate();
    let dave = Keys::generate();

    let (controller, active, rx) = controller_for(&alice.public_key());
    let proj = controller.inbox_slot();

    feed_dm(
        &proj,
        &rx,
        &alice,
        &gift_wrapped_dm(&dave, &alice.public_key(), "to alice", 100),
    );
    assert_eq!(proj.snapshot().conversations.len(), 1);

    *active.lock().unwrap() = Some(bob.public_key().to_hex());
    controller.on_account_change();
    assert!(
        proj.snapshot().conversations.is_empty(),
        "Alice to Bob: inbox must be empty"
    );

    feed_dm(
        &proj,
        &rx,
        &bob,
        &gift_wrapped_dm(&dave, &bob.public_key(), "to bob", 200),
    );
    assert_eq!(proj.snapshot().conversations.len(), 1);

    *active.lock().unwrap() = Some(carol.public_key().to_hex());
    controller.on_account_change();
    assert!(
        proj.snapshot().conversations.is_empty(),
        "Bob to Carol: inbox must be empty"
    );
}
