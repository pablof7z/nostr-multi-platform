//! Unit tests for the DM inbox account-switch teardown.
//!
//! Verifies that [`super::DmInboxController::on_account_change`] clears the
//! projection when the active account changes, so the previous account's
//! decrypted DMs never leak into the new account's UI (GitHub issue #1138 —
//! cross-account privacy leak in `DmInboxProjection::snapshot()`).
//!
//! ADR-0050 §D6: the controller now detects changes from the pubkey-only
//! `ActiveAccountSlot` (not a `Keys` slot), and the projection decrypts through
//! the signer port. The tests therefore drive the active-pubkey slot for the
//! switch and drain the emitted `Nip44DecryptForAccount` commands with a local
//! decryptor (the actor's local-backend behaviour) to land messages.

use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::{ActorCommand, ActorMail, CommandSender};
use nmp_nip17::DmInboxProjection;
use nostr::{EventBuilder, JsonUtil, Keys, Kind, PublicKey, Tag, Timestamp};

/// kind:1059 NIP-59 gift-wrap (matches `nmp_nip59::KIND_GIFT_WRAP`).
const KIND_GIFT_WRAP: u32 = 1059;

use super::DmInboxController;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Build a gift-wrapped kind:14 DM from `sender` to `receiver`.
fn gift_wrapped_dm(sender: &Keys, receiver: &PublicKey, content: &str, ts: u64) -> String {
    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(vec![Tag::public_key(*receiver)])
        .custom_created_at(Timestamp::from(ts))
        .build(sender.public_key());
    nmp_nip59::gift_wrap_local(sender, receiver, &rumor, Timestamp::from(ts))
        .expect("gift wrap succeeds")
        .as_json()
}

/// Feed one gift-wrap envelope into the projection via `ingest_gift_wrap`,
/// then drain the emitted port decrypts with `receiver_keys` (the
/// active local account) so the chain completes and the message lands.
fn feed_dm(proj: &DmInboxProjection, rx: &Receiver<ActorMail>, receiver_keys: &Keys, envelope: &str) {
    proj.ingest_gift_wrap(envelope, None);
    drive_decrypts(rx, receiver_keys);
}

/// Drain queued `Nip44DecryptForAccount` commands, decrypting locally with
/// `keys` and invoking each continuation (mirrors the actor's local dispatch
/// arm). Each continuation may enqueue the next chain step, so this walks the
/// outer→seal→store chain to completion.
fn drive_decrypts(rx: &Receiver<ActorMail>, keys: &Keys) {
    while let Ok(mail) = rx.try_recv() {
        let ActorMail::Command(ActorCommand::Nip44DecryptForAccount {
            peer_pubkey,
            ciphertext,
            continuation,
            ..
        }) = mail
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

/// Build a controller whose active account is `pubkey`, returning it, the
/// shared active-pubkey slot (to drive switches), and the command receiver (to
/// drain decrypts).
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

// ─── tests ───────────────────────────────────────────────────────────────────

/// Baseline: the DM leak that this fix addresses. After switching from Alice to
/// Bob the snapshot must be empty.
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

    // Switch active account to Bob.
    *active.lock().unwrap() = Some(bob.public_key().to_hex());
    let changed = controller.on_account_change();
    assert!(changed, "on_account_change must return true when the pubkey changed");

    assert!(
        proj.snapshot().conversations.is_empty(),
        "after account switch, snapshot must be empty — previous account's DMs must NOT appear"
    );
}

/// After the switch, new DMs addressed to the new account are accepted.
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

    // Switch to Bob.
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

/// Sign-out (active → None) also clears the inbox.
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
    assert_eq!(proj.snapshot().conversations.len(), 1, "sanity: DM ingested");

    // Sign out.
    *active.lock().unwrap() = None;
    let changed = controller.on_account_change();
    assert!(changed, "sign-out must be detected as a change");

    assert!(
        proj.snapshot().conversations.is_empty(),
        "sign-out must clear the inbox"
    );
}

/// No-op when the active account does not change.
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
    assert_eq!(proj.snapshot().conversations.len(), 1, "sanity: DM ingested");

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

/// Consecutive switches (Alice → Bob → Carol) each clear correctly.
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

    // Switch to Bob — Alice's messages must be cleared.
    *active.lock().unwrap() = Some(bob.public_key().to_hex());
    controller.on_account_change();
    assert!(
        proj.snapshot().conversations.is_empty(),
        "Alice→Bob: inbox must be empty"
    );

    feed_dm(
        &proj,
        &rx,
        &bob,
        &gift_wrapped_dm(&dave, &bob.public_key(), "to bob", 200),
    );
    assert_eq!(proj.snapshot().conversations.len(), 1);

    // Switch to Carol — Bob's messages must be cleared.
    *active.lock().unwrap() = Some(carol.public_key().to_hex());
    controller.on_account_change();
    assert!(
        proj.snapshot().conversations.is_empty(),
        "Bob→Carol: inbox must be empty"
    );
}
