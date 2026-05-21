//! Rumor field-preservation round-trip.
//!
//! `round_trip.rs` and `structure.rs` cover `kind`, `content`, `pubkey`, and
//! the outer envelope shape, but two fields the upcoming signer-seam refactor
//! relies on are not explicitly asserted to survive the wrap → unwrap cycle:
//!
//! - `created_at` (the rumor timestamp — distinct from the outer event's
//!   randomised gift-wrap timestamp; consumers tag DMs / Welcomes by this).
//! - The inner rumor's `tags` (Marmot Welcome rumors carry `e` / `relays`
//!   tags; NIP-17 DM rumors carry `p` tags addressing the recipient — losing
//!   them silently would route messages incorrectly).
//!
//! These pin the contract from inside the crate so a refactor cannot regress
//! it without a red test.

use nmp_nip59::{gift_wrap, unwrap_gift_wrap};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

/// Sentinel `created_at` chosen far in the past so it cannot be confused
/// with the gift-wrap envelope's randomised `now() - 0..172800` timestamp.
const RUMOR_SENTINEL_TS: u64 = 1_600_000_000;

#[test]
fn round_trip_preserves_rumor_created_at() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let rumor = EventBuilder::text_note("timestamp pin")
        .custom_created_at(Timestamp::from(RUMOR_SENTINEL_TS))
        .build(alice.public_key());

    let wrapped = gift_wrap(&alice, &bob.public_key(), rumor.clone(), None)
        .expect("gift_wrap should succeed");
    let unwrapped = unwrap_gift_wrap(&bob, &wrapped).expect("unwrap should succeed");

    assert_eq!(
        unwrapped.rumor.created_at,
        Timestamp::from(RUMOR_SENTINEL_TS),
        "rumor created_at must survive the wrap → unwrap cycle byte-for-byte"
    );
    // And it must be distinct from the outer envelope's timestamp — defends
    // against a regression where the rumor adopts the envelope's randomised
    // value.
    assert_ne!(
        unwrapped.rumor.created_at, wrapped.created_at,
        "rumor created_at must NOT equal the outer gift-wrap created_at"
    );
}

#[test]
fn round_trip_preserves_rumor_tags() {
    // NIP-17 DM rumors carry a recipient `p` tag plus a `subject` tag; Marmot
    // Welcome rumors carry `relays` and `e` tags. Build a representative
    // multi-tag rumor and assert every tag survives unchanged.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let charlie = Keys::generate();

    let tags = vec![
        Tag::public_key(bob.public_key()),
        Tag::public_key(charlie.public_key()),
        Tag::custom(
            nostr::TagKind::Custom("subject".into()),
            vec!["welcome-to-the-group".to_string()],
        ),
        Tag::custom(
            nostr::TagKind::Custom("relays".into()),
            vec![
                "wss://relay.example".to_string(),
                "wss://relay.test".to_string(),
            ],
        ),
    ];

    let rumor = EventBuilder::new(Kind::PrivateDirectMessage, "hi from alice")
        .tags(tags.clone())
        .custom_created_at(Timestamp::from(RUMOR_SENTINEL_TS))
        .build(alice.public_key());

    let wrapped = gift_wrap(&alice, &bob.public_key(), rumor.clone(), None)
        .expect("gift_wrap should succeed");
    let unwrapped = unwrap_gift_wrap(&bob, &wrapped).expect("unwrap should succeed");

    // Tag count must match — no tag may be added, dropped, or reordered.
    let unwrapped_tags: Vec<&Tag> = unwrapped.rumor.tags.iter().collect();
    let original_tags: Vec<&Tag> = rumor.tags.iter().collect();
    assert_eq!(
        unwrapped_tags.len(),
        original_tags.len(),
        "tag count must round-trip; got {unwrapped_tags:?}"
    );
    for (expected, actual) in original_tags.iter().zip(unwrapped_tags.iter()) {
        assert_eq!(
            (*expected).clone().to_vec(),
            (*actual).clone().to_vec(),
            "every tag must round-trip byte-for-byte"
        );
    }
}

#[test]
fn round_trip_preserves_rumor_kind_content_created_at_and_tags_together() {
    // Belt-and-suspenders: the four fields the prompt called out, asserted on
    // a single rumor so a regression that breaks ONLY when fields are
    // combined (e.g. JSON ordering bug) cannot slip through.
    let alice = Keys::generate();
    let bob = Keys::generate();

    let welcome_kind = Kind::from(444u16); // Marmot Welcome.
    let content = "welcome-bundle-cbor-here";
    let tags = vec![Tag::custom(
        nostr::TagKind::Custom("group-id".into()),
        vec!["abc-123".to_string()],
    )];

    let rumor = EventBuilder::new(welcome_kind, content)
        .tags(tags.clone())
        .custom_created_at(Timestamp::from(RUMOR_SENTINEL_TS))
        .build(alice.public_key());

    let wrapped =
        gift_wrap(&alice, &bob.public_key(), rumor, None).expect("gift_wrap should succeed");
    let unwrapped = unwrap_gift_wrap(&bob, &wrapped).expect("unwrap should succeed");

    assert_eq!(unwrapped.rumor.kind, welcome_kind, "kind must round-trip");
    assert_eq!(unwrapped.rumor.content, content, "content must round-trip");
    assert_eq!(
        unwrapped.rumor.created_at,
        Timestamp::from(RUMOR_SENTINEL_TS),
        "created_at must round-trip"
    );
    let unwrapped_tags: Vec<&Tag> = unwrapped.rumor.tags.iter().collect();
    assert_eq!(unwrapped_tags.len(), 1, "tag count must round-trip");
    assert_eq!(
        unwrapped_tags[0].clone().to_vec(),
        tags[0].clone().to_vec(),
        "tag content must round-trip byte-for-byte"
    );
}
