#![cfg(feature = "long-form")]
//! `try_from_event` on a fully-populated kind:30023 — every NIP-23 tag set.

mod common;

use common::stored;
use nmp_nip23::try_from_event;

#[test]
fn full_article_decodes_every_field_as_some() {
    let event = stored(
        &"a".repeat(64),
        &"b".repeat(64),
        30023,
        1_700_000_000,
        vec![
            vec!["d".into(), "long-form-intro".into()],
            vec!["title".into(), "On Decoders".into()],
            vec!["image".into(), "https://example.com/cover.png".into()],
            vec!["summary".into(), "Why decoders beat wrappers.".into()],
            vec!["published_at".into(), "1690000000".into()],
            vec!["t".into(), "rust".into()],
        ],
        "# heading\n\nbody",
    );

    let record = try_from_event(&event).expect("full article decodes");
    assert_eq!(record.d_tag, "long-form-intro");
    assert_eq!(record.title.as_deref(), Some("On Decoders"));
    assert_eq!(record.image.as_deref(), Some("https://example.com/cover.png"));
    assert_eq!(record.summary.as_deref(), Some("Why decoders beat wrappers."));
    assert_eq!(record.published_at, Some(1_690_000_000));
    assert_eq!(record.content, "# heading\n\nbody");
    // Unknown tags are preserved verbatim — apps that need `t` topic tags or
    // `r` URL references can read them without re-fetching from the store.
    assert!(record.tags.iter().any(|t| t.first().map(String::as_str) == Some("t")));
}
