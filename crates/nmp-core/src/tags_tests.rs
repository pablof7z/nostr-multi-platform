//! Unit tests for shared, kind-agnostic tag constructors and readers.
//!
//! Extracted from the inline `mod tests` in `tags.rs` to keep that file under
//! the 500-line ceiling (same precedent as `tags_reply_tests.rs`). `use
//! super::*` provides the same namespace access the inline tests had.

use super::*;

// ── constructors ────────────────────────────────────────────────────────

#[test]
fn e_tag_bare_is_two_columns() {
    assert_eq!(e_tag("abc", None, None), vec!["e", "abc"]);
}

#[test]
fn e_tag_with_relay_only() {
    assert_eq!(
        e_tag("abc", Some("wss://r.x"), None),
        vec!["e", "abc", "wss://r.x"]
    );
}

#[test]
fn e_tag_with_marker_forces_empty_relay_slot() {
    assert_eq!(
        e_tag("abc", None, Some("reply")),
        vec!["e", "abc", "", "reply"]
    );
}

#[test]
fn e_tag_with_relay_and_marker_is_four_columns() {
    assert_eq!(
        e_tag("abc", Some("wss://r.x"), Some("root")),
        vec!["e", "abc", "wss://r.x", "root"]
    );
}

#[test]
fn p_tag_with_and_without_relay() {
    assert_eq!(p_tag("pk", None), vec!["p", "pk"]);
    assert_eq!(p_tag("pk", Some("wss://r")), vec!["p", "pk", "wss://r"]);
}

#[test]
fn a_tag_builds_coordinate() {
    assert_eq!(
        a_tag(30023, "alice", "intro", None),
        vec!["a", "30023:alice:intro"]
    );
    assert_eq!(
        a_tag(30023, "alice", "intro", Some("wss://r")),
        vec!["a", "30023:alice:intro", "wss://r"]
    );
}

#[test]
fn q_tag_with_and_without_relay() {
    assert_eq!(q_tag("id", None), vec!["q", "id"]);
    assert_eq!(q_tag("id", Some("wss://r")), vec!["q", "id", "wss://r"]);
}

// ── readers ─────────────────────────────────────────────────────────────

#[test]
fn first_tag_value_and_all_tag_values() {
    let tags = vec![
        vec!["e".into(), "one".into()],
        vec!["e".into(), "two".into()],
        vec!["p".into(), "pk".into()],
    ];
    assert_eq!(first_tag_value(&tags, "e"), Some("one"));
    assert_eq!(all_tag_values(&tags, "e"), vec!["one", "two"]);
    assert_eq!(first_tag_value(&tags, "x"), None);
    assert!(all_tag_values(&tags, "x").is_empty());
}

#[test]
fn first_tag_value_handles_key_only_tag() {
    let tags = vec![vec!["e".into()]];
    assert_eq!(first_tag_value(&tags, "e"), None);
}
