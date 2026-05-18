//! `try_from_event` must reject any kind other than 30023.

mod common;

use common::stored;
use nmp_nip23::try_from_event;

#[test]
fn kind_1_returns_none_even_with_d_tag() {
    let event = stored(
        &"a".repeat(64),
        &"b".repeat(64),
        1,
        0,
        vec![vec!["d".into(), "intro".into()]],
        "body",
    );
    assert!(try_from_event(&event).is_none());
}

#[test]
fn kind_30024_draft_returns_none() {
    // Drafts (kind 30024) are NIP-37's domain, not ours.
    let event = stored(
        &"a".repeat(64),
        &"b".repeat(64),
        30024,
        0,
        vec![vec!["d".into(), "intro".into()]],
        "draft body",
    );
    assert!(try_from_event(&event).is_none());
}

#[test]
fn kind_zero_returns_none() {
    // Profile metadata is NIP-01.
    let event = stored(
        &"a".repeat(64),
        &"b".repeat(64),
        0,
        0,
        vec![vec!["d".into(), "intro".into()]],
        "{}",
    );
    assert!(try_from_event(&event).is_none());
}
