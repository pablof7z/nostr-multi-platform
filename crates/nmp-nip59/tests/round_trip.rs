//! Round-trip integration test: Alice gift_wraps a rumor to Bob; Bob
//! unwrap_gift_wrap recovers the identical rumor and the correct sender.
//!
//! Exit gate: this test is the headless integration proof for nmp-nip59.
//!
//! Note: `gift_wrap_with_expiration` (covered the legacy `expiration:
//! Option<Timestamp>` parameter) was removed when `gift_wrap` was tightened
//! to `pub(crate)` and the public surface migrated to
//! `gift_wrap_with_signer`. The new surface has no expiration parameter
//! (no production caller used it); restoring that feature is a deliberate
//! API change tracked outside this migration.

use std::sync::Arc;

use nostr::{EventBuilder, Keys, Kind, Timestamp, UnsignedEvent};
use nmp_nip59::{gift_wrap_with_signer, unwrap_gift_wrap, SignerForSeal, GIFT_WRAP_TOTAL_TIMEOUT};

fn alice_keys() -> Keys {
    Keys::parse("6b911fd37cdf5c81d4c0adb1ab7fa822ed253ab0ad9aa18d77257c88b29b718e").unwrap()
}

fn bob_keys() -> Keys {
    Keys::parse("7b911fd37cdf5c81d4c0adb1ab7fa822ed253ab0ad9aa18d77257c88b29b718e").unwrap()
}

/// Build an UnsignedEvent rumor attributed to Alice (kind:1 text note).
fn make_rumor(alice: &Keys) -> UnsignedEvent {
    EventBuilder::text_note("Hello, Bob! This is a secret.").build(alice.public_key())
}

/// Test-only shorthand: wrap `rumor` for `receiver` via the
/// `SignerForSeal` seam. `Keys` has a blanket impl that resolves every
/// `SignerOp` synchronously, so `.wait` returns immediately.
fn wrap(sender: &Keys, receiver: &nostr::PublicKey, rumor: &UnsignedEvent) -> nostr::Event {
    let signer: Arc<dyn SignerForSeal> = Arc::new(sender.clone());
    gift_wrap_with_signer(&signer, receiver, rumor, Timestamp::now())
        .wait(GIFT_WRAP_TOTAL_TIMEOUT)
        .expect("gift_wrap_with_signer should succeed")
}

#[test]
fn gift_wrap_round_trip() {
    let alice = alice_keys();
    let bob = bob_keys();
    let rumor = make_rumor(&alice);

    // Alice wraps the rumor for Bob.
    let wrapped = wrap(&alice, &bob.public_key(), &rumor);

    // Verify the outer envelope is kind:1059 (GiftWrap).
    assert_eq!(wrapped.kind, Kind::GiftWrap, "outer event must be kind 1059");

    // Bob unwraps.
    let unwrapped = unwrap_gift_wrap(&bob, &wrapped)
        .expect("unwrap_gift_wrap should succeed");

    // Sender must be Alice.
    assert_eq!(unwrapped.sender, alice.public_key(), "sender must be Alice's pubkey");

    // Rumor content and kind must match.
    assert_eq!(unwrapped.rumor.kind, rumor.kind, "rumor kind must round-trip");
    assert_eq!(unwrapped.rumor.content, rumor.content, "rumor content must round-trip");
    assert_eq!(unwrapped.rumor.pubkey, rumor.pubkey, "rumor pubkey must round-trip");
}

#[test]
fn wrong_key_cannot_unwrap() {
    let alice = alice_keys();
    let bob = bob_keys();
    let charlie = Keys::parse("5b911fd37cdf5c81d4c0adb1ab7fa822ed253ab0ad9aa18d77257c88b29b718e").unwrap();

    let rumor = make_rumor(&alice);
    let wrapped = wrap(&alice, &bob.public_key(), &rumor);

    // Charlie (wrong key) must fail to unwrap.
    let result = unwrap_gift_wrap(&charlie, &wrapped);
    assert!(result.is_err(), "unwrapping with wrong key must fail");
}
