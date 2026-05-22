#![cfg(feature = "long-form")]
//! A kind:30023 event without a `d` tag fails to decode — NIP-33 requires the
//! identifier for any parameterized-replaceable event.

mod common;

use common::stored;
use nmp_nip23::try_from_event;

#[test]
fn missing_d_tag_returns_none() {
    let event = stored(
        &"a".repeat(64),
        &"b".repeat(64),
        30023,
        0,
        vec![
            vec!["title".into(), "No D Tag Here".into()],
            vec!["t".into(), "rust".into()],
        ],
        "body",
    );
    assert!(try_from_event(&event).is_none());
}

#[test]
fn empty_tags_returns_none() {
    let event = stored(
        &"a".repeat(64),
        &"b".repeat(64),
        30023,
        0,
        Vec::new(),
        "body",
    );
    assert!(try_from_event(&event).is_none());
}
