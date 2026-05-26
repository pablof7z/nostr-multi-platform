//! Structural tests for NIP-59 gift-wrap envelopes.
//!
//! These complement `round_trip.rs` by asserting the *shape* of the outer
//! kind:1059 event — recipient `p`-tag, ephemeral signer, randomised
//! `created_at` — and by exercising content edge cases through the
//! wrap/unwrap round-trip (empty content, special characters, newlines).
//!
//! Migration note: post-`pub(crate)` tightening of `gift_wrap`, these
//! tests go through `gift_wrap_with_signer` via the synchronous `Keys`
//! blanket impl. The randomised-timestamp tweak that the legacy
//! `EventBuilder::gift_wrap` applied internally is now the caller's
//! responsibility — the `wrap` helper below passes
//! `Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK)` to preserve test
//! semantics 1:1.

use std::sync::Arc;

use nmp_nip59::{gift_wrap_with_signer, unwrap_gift_wrap, SignerForSeal, GIFT_WRAP_TOTAL_TIMEOUT};
use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
use nostr::{EventBuilder, Keys, Kind, Tag, TagKind, Timestamp, UnsignedEvent};

/// A rumor whose `created_at` is pinned far outside the now±2-day window so
/// timestamp-randomisation assertions are deterministic and never flaky.
const RUMOR_SENTINEL_TS: u64 = 1_600_000_000; // 2020-09-13, well in the past.

/// Build an UnsignedEvent rumor with a fixed `created_at` sentinel.
fn pinned_rumor(author: &Keys, content: &str) -> UnsignedEvent {
    EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(RUMOR_SENTINEL_TS))
        .build(author.public_key())
}

/// Test-only shorthand. The blanket `SignerForSeal` impl on `Keys`
/// resolves every step synchronously; `.wait` returns immediately.
fn wrap(sender: &Keys, receiver: &nostr::PublicKey, rumor: &UnsignedEvent) -> nostr::Event {
    let signer: Arc<dyn SignerForSeal> = Arc::new(sender.clone());
    let tweaked = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
    gift_wrap_with_signer(&signer, receiver, rumor, tweaked)
        .wait(GIFT_WRAP_TOTAL_TIMEOUT)
        .expect("gift_wrap_with_signer should succeed")
}

#[test]
fn gift_wrap_outer_event_is_kind_1059() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let rumor = pinned_rumor(&alice, "structural check");

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);

    assert_eq!(
        wrapped.kind,
        Kind::GiftWrap,
        "outer envelope must be kind:1059"
    );
    assert_eq!(u16::from(wrapped.kind), 1059, "GiftWrap numeric kind is 1059");
}

#[test]
fn gift_wrap_carries_recipient_p_tag() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let rumor = pinned_rumor(&alice, "p-tag check");

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);

    // Exactly one `p`-tag, and it must address Bob.
    let p_tags: Vec<&Tag> = wrapped
        .tags
        .iter()
        .filter(|t| t.kind() == TagKind::p())
        .collect();
    assert_eq!(p_tags.len(), 1, "gift-wrap must carry exactly one p-tag");

    let expected = Tag::public_key(bob.public_key());
    assert_eq!(
        p_tags[0], &expected,
        "the p-tag must address the recipient's public key"
    );
}

#[test]
fn gift_wrap_p_tag_is_recipient_not_sender() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let rumor = pinned_rumor(&alice, "not-sender check");

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);

    let addressed_pubkey = wrapped
        .tags
        .iter()
        .find(|t| t.kind() == TagKind::p())
        .and_then(|t| t.content())
        .expect("p-tag with content must exist");

    assert_eq!(
        addressed_pubkey,
        bob.public_key().to_hex(),
        "p-tag must address the recipient"
    );
    assert_ne!(
        addressed_pubkey,
        alice.public_key().to_hex(),
        "p-tag must NOT leak the sender's identity"
    );
}

#[test]
fn gift_wrap_signed_by_ephemeral_key() {
    // NIP-59 mandates the outer event be signed by a one-time ephemeral key,
    // never the sender's real key — otherwise metadata leaks.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let rumor = pinned_rumor(&alice, "ephemeral check");

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);

    assert_ne!(
        wrapped.pubkey,
        alice.public_key(),
        "outer event must NOT be signed by the sender's key"
    );
    assert_ne!(
        wrapped.pubkey,
        bob.public_key(),
        "outer event must NOT be signed by the recipient's key"
    );
    // The signature must still be valid for whatever ephemeral key signed it.
    assert!(
        wrapped.verify().is_ok(),
        "gift-wrap signature must verify against its ephemeral signer"
    );
}

#[test]
fn gift_wrap_created_at_is_randomised_not_rumor_timestamp() {
    // NIP-59 requires the gift-wrap `created_at` be `now()` minus a random
    // offset of up to 2 days (RANGE_RANDOM_TIMESTAMP_TWEAK = 0..172800).
    // It must therefore differ from the rumor's own timestamp, which we pin
    // far in the past so the assertion is deterministic.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let rumor = pinned_rumor(&alice, "timestamp check");

    let before = Timestamp::now().as_secs();
    let wrapped = wrap(&alice, &bob.public_key(), &rumor);
    let after = Timestamp::now().as_secs();

    let wrapped_ts = wrapped.created_at.as_secs();

    // 1. The gift-wrap timestamp is NOT the rumor's timestamp.
    assert_ne!(
        wrapped_ts, RUMOR_SENTINEL_TS,
        "gift-wrap created_at must not equal the rumor's created_at"
    );
    assert_ne!(
        wrapped.created_at, rumor.created_at,
        "gift-wrap created_at must not equal the rumor's created_at"
    );

    // 2. It must sit within [now - 2 days, now]: tweaked() subtracts a random
    //    offset in 0..172800 from the current time.
    let slack = 5u64; // wall-clock slack for the now() reads above.
    assert!(
        wrapped_ts <= after + slack,
        "gift-wrap created_at ({wrapped_ts}) must not be in the future (after={after})"
    );
    assert!(
        wrapped_ts + 172_800 >= before.saturating_sub(slack),
        "gift-wrap created_at ({wrapped_ts}) must be within 2 days of now (before={before})"
    );
}

#[test]
fn gift_wrap_round_trip_empty_content() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let rumor = pinned_rumor(&alice, "");

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);
    let unwrapped =
        unwrap_gift_wrap(&bob, &wrapped).expect("unwrap of empty content should succeed");

    assert_eq!(
        unwrapped.rumor.content, "",
        "empty content must round-trip as empty"
    );
    assert_eq!(unwrapped.rumor.content, rumor.content);
    assert_eq!(unwrapped.sender, alice.public_key());
}

#[test]
fn gift_wrap_round_trip_special_characters_and_newlines() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    // Newlines, quotes, backslashes, emoji, unicode, tabs — all of which
    // must survive JSON serialisation inside the seal and gift-wrap layers.
    let tricky = "line1\nline2\t\"quoted\"\\backslash\r\nüñîçødé 🎁🔐 \0 end";
    let rumor = pinned_rumor(&alice, tricky);

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);
    let unwrapped =
        unwrap_gift_wrap(&bob, &wrapped).expect("unwrap of special content should succeed");

    assert_eq!(
        unwrapped.rumor.content, tricky,
        "special characters and newlines must round-trip byte-for-byte"
    );
}

#[test]
fn gift_wrap_round_trip_large_content() {
    // A multi-kilobyte payload exercises the NIP-44 chunking path.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let large = "A".repeat(16 * 1024);
    let rumor = pinned_rumor(&alice, &large);

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);
    let unwrapped =
        unwrap_gift_wrap(&bob, &wrapped).expect("unwrap of large content should succeed");

    assert_eq!(unwrapped.rumor.content.len(), large.len());
    assert_eq!(unwrapped.rumor.content, large);
}

#[test]
fn gift_wrap_preserves_non_default_rumor_kind() {
    // NIP-59 carries arbitrary inner kinds — kind:444 (Marmot Welcome) is the
    // load-bearing case for this crate. The rumor kind must survive the wrap.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let welcome_kind = Kind::from(444u16);
    let rumor = EventBuilder::new(welcome_kind, "mls-welcome-payload")
        .custom_created_at(Timestamp::from(RUMOR_SENTINEL_TS))
        .build(alice.public_key());

    let wrapped = wrap(&alice, &bob.public_key(), &rumor);
    let unwrapped = unwrap_gift_wrap(&bob, &wrapped).expect("unwrap should succeed");

    assert_eq!(
        unwrapped.rumor.kind, welcome_kind,
        "inner rumor kind (444 Welcome) must round-trip unchanged"
    );
}
