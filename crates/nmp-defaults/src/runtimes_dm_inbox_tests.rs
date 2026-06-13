//! Unit tests for the DM inbox account-switch teardown.
//!
//! Verifies that [`super::DmInboxController::on_account_change`] clears the
//! projection when the active account changes, so the previous account's
//! decrypted DMs never leak into the new account's UI (GitHub issue #1138 —
//! cross-account privacy leak in `DmInboxProjection::snapshot()`).
//!
//! The tests drive `DmInboxController` directly (same pattern as
//! `runtimes_zap_tests.rs` driving `ZapReceiptsRuntimeController`), so no
//! real `NmpApp` is needed: the controller's public interface is sufficient
//! to reproduce the before/after states.

use std::sync::{Arc, Mutex};

use nmp_nip17::DmInboxProjection;
use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp};

/// kind:1059 NIP-59 gift-wrap (matches `nmp_nip59::KIND_GIFT_WRAP`).
const KIND_GIFT_WRAP: u32 = 1059;

use super::DmInboxController;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Build a gift-wrapped kind:14 DM from `sender` to `receiver`.
fn gift_wrapped_dm(sender: &Keys, receiver: &nostr::PublicKey, content: &str, ts: u64) -> String {
    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(vec![Tag::public_key(*receiver)])
        .custom_created_at(Timestamp::from(ts))
        .build(sender.public_key());
    nmp_nip59::gift_wrap_local(sender, receiver, &rumor, Timestamp::from(ts))
        .expect("gift wrap succeeds")
        .as_json()
}

/// Feed one gift-wrap envelope into the projection via its `RawEventObserver`
/// interface (the same path live relay delivery uses).
fn feed_dm(proj: &DmInboxProjection, envelope: &str) {
    use nmp_core::RawEventObserver as _;
    proj.on_raw_event(KIND_GIFT_WRAP, envelope);
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// Baseline: the DM leak that this fix addresses.
///
/// Without the fix, after switching from Alice to Bob the inbox still
/// holds Alice's decrypted messages, so `snapshot()` leaks them to Bob's UI.
/// This test proves the fix: after `on_account_change` the snapshot must be
/// empty.
#[test]
fn account_switch_clears_previous_accounts_messages() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let carol = Keys::generate();

    // Start signed in as Alice.
    let local_keys: Arc<Mutex<Option<Keys>>> = Arc::new(Mutex::new(Some(alice.clone())));
    let controller = DmInboxController::new(Arc::clone(&local_keys));

    // Inject a DM addressed to Alice into the projection.
    let proj = controller.inbox_slot();
    let envelope = gift_wrapped_dm(&carol, &alice.public_key(), "secret for alice", 100);
    feed_dm(&proj, &envelope);

    // Confirm Alice sees the message.
    let snap_before = proj.snapshot();
    assert_eq!(
        snap_before.conversations.len(),
        1,
        "Alice should see the DM before account switch"
    );

    // Switch active account to Bob.
    *local_keys.lock().unwrap() = Some(bob.clone());

    // Trigger the account-change callback (the fix under test).
    let changed = controller.on_account_change();
    assert!(
        changed,
        "on_account_change must return true when the pubkey changed"
    );

    // After the switch, the inbox must be empty — Alice's messages must not
    // leak into Bob's view. This was the privacy bug before the fix.
    let snap_after = proj.snapshot();
    assert!(
        snap_after.conversations.is_empty(),
        "after account switch, snapshot must be empty — previous account's \
         DMs must NOT appear: got {:?}",
        snap_after.conversations
    );
}

/// After the switch, new DMs addressed to the new account are accepted.
#[test]
fn new_account_receives_its_own_dms_after_switch() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let carol = Keys::generate();

    let local_keys: Arc<Mutex<Option<Keys>>> = Arc::new(Mutex::new(Some(alice.clone())));
    let controller = DmInboxController::new(Arc::clone(&local_keys));

    // Inject a DM for Alice.
    let proj = controller.inbox_slot();
    feed_dm(
        &proj,
        &gift_wrapped_dm(&carol, &alice.public_key(), "for alice", 200),
    );

    // Switch to Bob.
    *local_keys.lock().unwrap() = Some(bob.clone());
    controller.on_account_change();

    // Inject a DM for Bob after the switch.
    feed_dm(
        &proj,
        &gift_wrapped_dm(&carol, &bob.public_key(), "for bob", 300),
    );

    // Snapshot should contain only Bob's DM.
    let snap = proj.snapshot();
    assert_eq!(snap.conversations.len(), 1, "Bob should see his DM");
    assert_eq!(
        snap.conversations[0].messages[0].content, "for bob",
        "the message must be Bob's, not Alice's"
    );
}

/// Sign-out (active → None) also clears the inbox, so a guest view or
/// the next sign-in does not see the previous account's DMs.
#[test]
fn sign_out_clears_inbox() {
    let alice = Keys::generate();
    let carol = Keys::generate();

    let local_keys: Arc<Mutex<Option<Keys>>> = Arc::new(Mutex::new(Some(alice.clone())));
    let controller = DmInboxController::new(Arc::clone(&local_keys));

    // Inject a DM for Alice.
    let proj = controller.inbox_slot();
    feed_dm(
        &proj,
        &gift_wrapped_dm(&carol, &alice.public_key(), "private", 100),
    );
    assert_eq!(
        proj.snapshot().conversations.len(),
        1,
        "sanity: DM ingested"
    );

    // Sign out.
    *local_keys.lock().unwrap() = None;
    let changed = controller.on_account_change();
    assert!(changed, "sign-out must be detected as a change");

    let snap = proj.snapshot();
    assert!(
        snap.conversations.is_empty(),
        "sign-out must clear the inbox; got {:?}",
        snap.conversations
    );
}

/// No-op when the active account does not change (same pubkey on consecutive
/// calls). The projection must NOT be cleared.
#[test]
fn no_change_does_not_clear_projection() {
    let alice = Keys::generate();
    let carol = Keys::generate();

    let local_keys: Arc<Mutex<Option<Keys>>> = Arc::new(Mutex::new(Some(alice.clone())));
    let controller = DmInboxController::new(Arc::clone(&local_keys));

    // Inject a DM for Alice.
    let proj = controller.inbox_slot();
    feed_dm(
        &proj,
        &gift_wrapped_dm(&carol, &alice.public_key(), "for alice", 100),
    );
    assert_eq!(
        proj.snapshot().conversations.len(),
        1,
        "sanity: DM ingested"
    );

    // Call on_account_change without actually changing the active account.
    let changed = controller.on_account_change();
    assert!(
        !changed,
        "on_account_change must return false when the pubkey is unchanged"
    );

    // The DM is still visible — the projection was preserved.
    let snap = proj.snapshot();
    assert_eq!(
        snap.conversations.len(),
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

    let local_keys: Arc<Mutex<Option<Keys>>> = Arc::new(Mutex::new(Some(alice.clone())));
    let controller = DmInboxController::new(Arc::clone(&local_keys));
    let proj = controller.inbox_slot();

    // Alice receives a DM.
    feed_dm(
        &proj,
        &gift_wrapped_dm(&dave, &alice.public_key(), "to alice", 100),
    );
    assert_eq!(proj.snapshot().conversations.len(), 1);

    // Switch to Bob — Alice's messages must be cleared.
    *local_keys.lock().unwrap() = Some(bob.clone());
    controller.on_account_change();
    assert!(
        proj.snapshot().conversations.is_empty(),
        "Alice→Bob: inbox must be empty"
    );

    // Bob receives a DM.
    feed_dm(
        &proj,
        &gift_wrapped_dm(&dave, &bob.public_key(), "to bob", 200),
    );
    assert_eq!(proj.snapshot().conversations.len(), 1);

    // Switch to Carol — Bob's messages must be cleared.
    *local_keys.lock().unwrap() = Some(carol.clone());
    controller.on_account_change();
    assert!(
        proj.snapshot().conversations.is_empty(),
        "Bob→Carol: inbox must be empty"
    );
}
