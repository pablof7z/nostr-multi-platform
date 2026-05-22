#![cfg(feature = "long-form")]
//! `Article::new(...).title(...).build() → fake-stored → try_from_event` round
//! trip. Confirms the encode/decode pair preserves every field with no loss.
//!
//! The `UnsignedEvent` would normally be signed → wire-encoded → re-parsed to
//! a `StoredEvent`. For the contract test we simulate that path by lifting the
//! `UnsignedEvent`'s fields into a `StoredEvent` with a placeholder id/sig.

mod common;

use common::stored;
use nmp_nip23::{try_from_event, Article};

#[test]
fn builder_decoded_record_matches_inputs() {
    let unsigned = Article::new("intro")
        .title("Hello, World")
        .image("https://example.com/img.png")
        .summary("A brief greeting.")
        .published_at(1_690_000_000)
        .content("# Hi\n\nbody")
        .build("b".repeat(64), 1_700_000_000)
        .expect("build accepts a populated article");

    // Simulate the signer → store round trip by lifting the UnsignedEvent into
    // a StoredEvent with the same fields.
    let event = stored(
        &"a".repeat(64),
        &unsigned.pubkey,
        unsigned.kind,
        unsigned.created_at,
        unsigned.tags.clone(),
        &unsigned.content,
    );

    let record = try_from_event(&event).expect("round-trip decodes");
    assert_eq!(record.d_tag, "intro");
    assert_eq!(record.title.as_deref(), Some("Hello, World"));
    assert_eq!(record.image.as_deref(), Some("https://example.com/img.png"));
    assert_eq!(record.summary.as_deref(), Some("A brief greeting."));
    assert_eq!(record.published_at, Some(1_690_000_000));
    assert_eq!(record.content, "# Hi\n\nbody");
    assert_eq!(record.created_at, 1_700_000_000);
    assert_eq!(record.author, "b".repeat(64));
}

#[test]
fn builder_minimal_round_trip_only_d_tag() {
    let unsigned = Article::new("intro")
        .content("just the body")
        .build("b".repeat(64), 1)
        .unwrap();

    let event = stored(
        &"a".repeat(64),
        &unsigned.pubkey,
        unsigned.kind,
        unsigned.created_at,
        unsigned.tags,
        &unsigned.content,
    );

    let record = try_from_event(&event).unwrap();
    assert_eq!(record.d_tag, "intro");
    assert_eq!(record.title, None);
    assert_eq!(record.published_at, None);
    assert_eq!(record.content, "just the body");
}
