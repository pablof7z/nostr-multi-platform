#![cfg(feature = "long-form")]
//! `try_from_event` on a minimal kind:30023 — only `d` tag + content.

mod common;

use common::stored;
use nmp_nip23::try_from_event;

#[test]
fn minimal_article_decodes_with_none_for_optional_fields() {
    let event = stored(
        &"a".repeat(64),
        &"b".repeat(64),
        30023,
        1_700_000_000,
        vec![vec!["d".into(), "intro".into()]],
        "the body",
    );

    let record = try_from_event(&event).expect("minimal article decodes");
    assert_eq!(record.d_tag, "intro");
    assert_eq!(record.content, "the body");
    assert_eq!(record.title, None);
    assert_eq!(record.image, None);
    assert_eq!(record.summary, None);
    assert_eq!(record.published_at, None);
    assert_eq!(record.created_at, 1_700_000_000);
    assert_eq!(record.event_id, "a".repeat(64));
    assert_eq!(record.author, "b".repeat(64));
}
