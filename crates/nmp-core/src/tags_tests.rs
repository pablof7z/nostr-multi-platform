//! Unit tests for the shared tag constructors / readers / NIP-10 parser and
//! the [`super::contact_follows`] follow-set extraction function.
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

// ── NIP-02 kind:3 contact-list edit builders (issue #1246) ──────────────

const PUBKEY_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PUBKEY_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PUBKEY_X: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn p(pk: &str) -> Vec<String> {
    vec!["p".to_string(), pk.to_string()]
}

#[test]
fn kind3_tags_after_add_appends_new_pubkey() {
    let current = vec![p(PUBKEY_A), p(PUBKEY_B)];
    let result = kind3_tags_after_add(&current, PUBKEY_X);
    assert_eq!(result, vec![p(PUBKEY_A), p(PUBKEY_B), p(PUBKEY_X)]);
}

#[test]
fn kind3_tags_after_add_is_idempotent() {
    // Adding a pubkey that is already present must not create a duplicate.
    let current = vec![p(PUBKEY_A), p(PUBKEY_B)];
    let result = kind3_tags_after_add(&current, PUBKEY_A);
    assert_eq!(
        result,
        vec![p(PUBKEY_A), p(PUBKEY_B)],
        "no duplicate inserted"
    );
}

#[test]
fn kind3_tags_after_add_to_empty_list() {
    let result = kind3_tags_after_add(&[], PUBKEY_A);
    assert_eq!(result, vec![p(PUBKEY_A)]);
}

#[test]
fn kind3_tags_after_add_preserves_non_p_tags_and_existing_columns() {
    // A non-`p` tag and a relay-hinted+petnamed `p` must survive an add, and
    // the new follow is appended after them.
    let current = vec![
        vec!["r".to_string(), "wss://relay".to_string()],
        vec![
            "p".to_string(),
            PUBKEY_A.to_string(),
            "wss://hint".to_string(),
            "alice".to_string(),
        ],
    ];
    let result = kind3_tags_after_add(&current, PUBKEY_X);
    assert_eq!(
        result,
        vec![
            vec!["r".to_string(), "wss://relay".to_string()],
            vec![
                "p".to_string(),
                PUBKEY_A.to_string(),
                "wss://hint".to_string(),
                "alice".to_string(),
            ],
            p(PUBKEY_X),
        ]
    );
}

#[test]
fn kind3_tags_after_remove_removes_target() {
    let current = vec![p(PUBKEY_A), p(PUBKEY_B), p(PUBKEY_X)];
    let result = kind3_tags_after_remove(&current, PUBKEY_B);
    assert_eq!(result, vec![p(PUBKEY_A), p(PUBKEY_X)]);
}

#[test]
fn kind3_tags_after_remove_is_idempotent() {
    // Removing a pubkey not in the list must return the list unchanged.
    let current = vec![p(PUBKEY_A), p(PUBKEY_B)];
    let result = kind3_tags_after_remove(&current, PUBKEY_X);
    assert_eq!(result, vec![p(PUBKEY_A), p(PUBKEY_B)]);
}

#[test]
fn kind3_tags_after_remove_from_empty_list() {
    let result = kind3_tags_after_remove(&[], PUBKEY_A);
    assert!(result.is_empty());
}

#[test]
fn kind3_tags_after_remove_drops_any_arity_and_keeps_non_p() {
    // A relay-hinted+petnamed `p` is removed by pubkey; non-`p` tags survive.
    let current = vec![
        vec!["r".to_string(), "wss://relay".to_string()],
        vec![
            "p".to_string(),
            PUBKEY_A.to_string(),
            "wss://hint".to_string(),
            "alice".to_string(),
        ],
        p(PUBKEY_B),
    ];
    let result = kind3_tags_after_remove(&current, PUBKEY_A);
    assert_eq!(
        result,
        vec![
            vec!["r".to_string(), "wss://relay".to_string()],
            p(PUBKEY_B)
        ]
    );
}

#[test]
fn kind3_tags_sequence_add_then_remove() {
    // Simulate a real add-X-then-remove-B sequence on [A, B]:
    // [A, B] → add X → [A, B, X] → remove B → [A, X]
    let start = vec![p(PUBKEY_A), p(PUBKEY_B)];
    let after_add = kind3_tags_after_add(&start, PUBKEY_X);
    assert_eq!(after_add, vec![p(PUBKEY_A), p(PUBKEY_B), p(PUBKEY_X)]);
    let after_remove = kind3_tags_after_remove(&after_add, PUBKEY_B);
    assert_eq!(after_remove, vec![p(PUBKEY_A), p(PUBKEY_X)]);
}
