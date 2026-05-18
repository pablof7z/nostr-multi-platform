//! Round-trip integration test: Alice gift_wraps a rumor to Bob; Bob
//! unwrap_gift_wrap recovers the identical rumor and the correct sender.
//!
//! Exit gate: this test is the headless integration proof for nmp-nip59.

use nostr::{EventBuilder, Keys, Kind, UnsignedEvent};
use nmp_nip59::{gift_wrap, unwrap_gift_wrap};

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

#[test]
fn gift_wrap_round_trip() {
    let alice = alice_keys();
    let bob = bob_keys();
    let rumor = make_rumor(&alice);

    // Alice wraps the rumor for Bob — no expiration.
    let wrapped = gift_wrap(&alice, &bob.public_key(), rumor.clone(), None)
        .expect("gift_wrap should succeed");

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
    let wrapped = gift_wrap(&alice, &bob.public_key(), rumor, None)
        .expect("gift_wrap should succeed");

    // Charlie (wrong key) must fail to unwrap.
    let result = unwrap_gift_wrap(&charlie, &wrapped);
    assert!(result.is_err(), "unwrapping with wrong key must fail");
}

#[test]
fn gift_wrap_with_expiration() {
    let alice = alice_keys();
    let bob = bob_keys();
    let rumor = make_rumor(&alice);
    let expiry = nostr::Timestamp::from(9_999_999_999u64);

    let wrapped = gift_wrap(&alice, &bob.public_key(), rumor.clone(), Some(expiry))
        .expect("gift_wrap with expiration should succeed");

    // The expiration tag must be present on the outer event.
    let has_expiry = wrapped.tags.iter().any(|t: &nostr::Tag| {
        t.kind() == nostr::TagKind::Expiration
    });
    assert!(has_expiry, "gift-wrap event must carry expiration tag");

    // Bob can still unwrap.
    let unwrapped = unwrap_gift_wrap(&bob, &wrapped)
        .expect("unwrap_gift_wrap with expiration should succeed");
    assert_eq!(unwrapped.rumor.content, rumor.content);
}

#[test]
fn register_populates_module_registry() {
    let mut registry = nmp_core::substrate::ModuleRegistry::default();
    nmp_nip59::register(&mut registry);

    let descriptors = registry.descriptors();
    // WelcomeWrap (Action) and WelcomeUnwrap (Domain) must be registered.
    let has_wrap_action = descriptors.iter().any(|d| d.namespace == "nip59.welcome_wrap");
    let has_unwrap_domain = descriptors.iter().any(|d| d.namespace == "nip59.welcome_unwrap");
    assert!(has_wrap_action, "WelcomeWrap action must be registered");
    assert!(has_unwrap_domain, "WelcomeUnwrap domain must be registered");
}
