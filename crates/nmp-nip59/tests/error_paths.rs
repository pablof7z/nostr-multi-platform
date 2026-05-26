//! Error-path tests for NIP-59 unwrap.
//!
//! These assert that malformed input and decryption failures return a typed
//! `Nip59Error` rather than panicking — silent crypto-wrapping bugs are the
//! primary risk this crate must guard against.

use std::sync::Arc;

use nmp_nip59::{
    gift_wrap_with_signer, unwrap_gift_wrap, Nip59Error, SignerForSeal, GIFT_WRAP_TOTAL_TIMEOUT,
};
use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
use nostr::{EventBuilder, Keys, Timestamp, UnsignedEvent};

const RUMOR_SENTINEL_TS: u64 = 1_600_000_000;

fn pinned_rumor(author: &Keys, content: &str) -> UnsignedEvent {
    EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(RUMOR_SENTINEL_TS))
        .build(author.public_key())
}

/// Test-only shorthand. The blanket `SignerForSeal` impl on `Keys`
/// resolves every step synchronously.
fn wrap(sender: &Keys, receiver: &nostr::PublicKey, rumor: &UnsignedEvent) -> nostr::Event {
    let signer: Arc<dyn SignerForSeal> = Arc::new(sender.clone());
    let tweaked = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
    gift_wrap_with_signer(&signer, receiver, rumor, tweaked)
        .wait(GIFT_WRAP_TOTAL_TIMEOUT)
        .expect("gift_wrap_with_signer should succeed")
}

#[test]
fn unwrap_non_gift_wrap_event_returns_not_gift_wrap() {
    // A plain signed kind:1 text note is not a gift-wrap; unwrap must reject
    // it with the typed `NotGiftWrap` variant — not panic, not a crypto error.
    let bob = Keys::generate();
    let plain = EventBuilder::text_note("just a normal note")
        .sign_with_keys(&bob)
        .expect("signing a kind:1 note should succeed");

    let result = unwrap_gift_wrap(&bob, &plain);

    assert!(
        matches!(result, Err(Nip59Error::NotGiftWrap)),
        "expected Err(NotGiftWrap) for a non-1059 event, got {result:?}"
    );
}

#[test]
fn unwrap_with_wrong_key_returns_err_not_panic() {
    // Charlie holds the wrong key: NIP-44 decryption of the outer layer
    // fails. The contract is "typed Err", and crucially: no panic.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let charlie = Keys::generate();
    let rumor = pinned_rumor(&alice, "secret for bob only");

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);

    let result = unwrap_gift_wrap(&charlie, &wrapped);

    assert!(
        result.is_err(),
        "unwrapping with the wrong key must return Err, got {result:?}"
    );
    // Wrong-key failure surfaces as a crypto error from nip44, mapped into
    // the catch-all `Nostr` variant — not `NotGiftWrap`/`SenderMismatch`.
    assert!(
        matches!(result, Err(Nip59Error::Nostr(_))),
        "wrong-key failure should map to Nip59Error::Nostr, got {result:?}"
    );
}

#[test]
fn unwrap_sender_cannot_decrypt_own_gift_wrap() {
    // The gift-wrap is NIP-44 encrypted to the *recipient's* key. The sender
    // does not hold that conversation key, so even Alice cannot unwrap what
    // she sent to Bob — confirming the envelope is genuinely recipient-bound.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let rumor = pinned_rumor(&alice, "for bob");

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);

    let result = unwrap_gift_wrap(&alice, &wrapped);
    assert!(
        result.is_err(),
        "the sender must not be able to unwrap their own gift-wrap, got {result:?}"
    );
}

#[test]
fn unwrap_tampered_content_returns_err() {
    // Flipping ciphertext bytes must be detected (NIP-44 is authenticated):
    // a tampered gift-wrap must fail to unwrap, never silently succeed.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let rumor = pinned_rumor(&alice, "integrity check");

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);

    // Rebuild the event JSON with a corrupted content field, re-sign with a
    // fresh ephemeral key so the outer signature stays valid but the
    // NIP-44 payload is garbage.
    let mut corrupted_content = wrapped.content.clone();
    // Mutate a character in the middle of the base64 payload.
    if let Some(mid) = corrupted_content.char_indices().nth(corrupted_content.len() / 2) {
        let (idx, ch) = mid;
        let replacement = if ch == 'A' { 'B' } else { 'A' };
        corrupted_content.replace_range(idx..idx + ch.len_utf8(), &replacement.to_string());
    }

    let ephemeral = Keys::generate();
    let tampered = EventBuilder::new(wrapped.kind, corrupted_content)
        .tags(wrapped.tags.iter().cloned())
        .custom_created_at(wrapped.created_at)
        .sign_with_keys(&ephemeral)
        .expect("re-signing the tampered envelope should succeed");

    let result = unwrap_gift_wrap(&bob, &tampered);
    assert!(
        result.is_err(),
        "a tampered gift-wrap must fail to unwrap, got {result:?}"
    );
}

#[test]
fn nip59_error_is_displayable_and_comparable() {
    // The error type backs user-facing diagnostics; Display must be
    // non-empty and PartialEq must hold for the typed variants.
    assert_eq!(Nip59Error::NotGiftWrap, Nip59Error::NotGiftWrap);
    assert_ne!(Nip59Error::NotGiftWrap, Nip59Error::SenderMismatch);

    assert!(!Nip59Error::NotGiftWrap.to_string().is_empty());
    assert!(!Nip59Error::SenderMismatch.to_string().is_empty());
    assert!(Nip59Error::Nostr("boom".into())
        .to_string()
        .contains("boom"));
}
