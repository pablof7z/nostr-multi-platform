//! #3091 — `InterestShape::addresses` correctness: the `from_filter_json`
//! `#a` decode and the `matches_event` coordinate predicate.
//!
//! Split out of `tests.rs` for file-size discipline (AGENTS.md 500 LOC hard
//! ceiling); shares no state with it beyond the public `interest` module
//! surface reached through `use super::*`.

use super::*;
use std::collections::BTreeSet;

/// Deterministic 64-char hex pubkey/event-id fixture from a single byte,
/// mirroring `tests.rs`'s helper of the same name.
fn hex(byte: &str) -> String {
    byte.repeat(32)
}

// ─── from_filter_json `#a` decode ──────────────────────────────────────────

#[test]
fn from_filter_json_decodes_hash_a_tag_into_addresses() {
    let json = format!(
        r##"{{"#a":["30023:{}:my-article","10002:{}:"]}}"##,
        hex("aa"),
        hex("bb"),
    );
    let shape = InterestShape::from_filter_json(&json).expect("valid object");

    assert_eq!(
        shape.addresses,
        [
            NaddrCoord {
                pubkey: hex("aa"),
                kind: 30_023,
                d_tag: "my-article".to_string(),
            },
            NaddrCoord {
                pubkey: hex("bb"),
                kind: 10_002,
                d_tag: String::new(),
            },
        ]
        .into_iter()
        .collect::<BTreeSet<_>>()
    );
    // `#a` must NOT also land in the opaque generic `tags` bucket.
    assert!(shape.tags.is_empty());
}

#[test]
fn from_filter_json_hash_a_tag_preserves_colon_in_d_tag() {
    // splitn(3, ':') captures everything after the second colon verbatim,
    // unlike `nostr::Coordinate::from_kpi_format` (which truncates at the
    // first extra colon) — this is the exact inverse of `filter_json_for`,
    // whose `Coordinate::to_string()` never escapes a `:` inside the
    // identifier.
    let json = format!(r##"{{"#a":["30023:{}:a:b:c"]}}"##, hex("aa"));
    let shape = InterestShape::from_filter_json(&json).expect("valid object");
    assert_eq!(
        shape.addresses,
        [NaddrCoord {
            pubkey: hex("aa"),
            kind: 30_023,
            d_tag: "a:b:c".to_string(),
        }]
        .into_iter()
        .collect()
    );
}

#[test]
fn from_filter_json_hash_a_tag_tolerates_malformed_entries() {
    // Missing the d-tag segment, non-numeric kind, and a well-formed entry
    // mixed in one array: the malformed ones are dropped, the valid one lands.
    let json = format!(
        r##"{{"#a":["30023:{}","notakind:{}:x","30023:{}:ok"]}}"##,
        hex("aa"),
        hex("bb"),
        hex("cc"),
    );
    let shape = InterestShape::from_filter_json(&json).expect("valid object");
    assert_eq!(
        shape.addresses,
        [NaddrCoord {
            pubkey: hex("cc"),
            kind: 30_023,
            d_tag: "ok".to_string(),
        }]
        .into_iter()
        .collect()
    );
}

// ─── matches_event `addresses` predicate ───────────────────────────────────

#[test]
fn matches_event_addresses_accepts_matching_coordinate() {
    let mut shape = InterestShape::default();
    shape.addresses.insert(NaddrCoord {
        pubkey: hex("aa"),
        kind: 30_023,
        d_tag: "my-article".to_string(),
    });

    // Correct kind + author + `["d", "my-article"]` tag → matches.
    assert!(shape.matches_event(
        &hex("aa"),
        30_023,
        100,
        &[vec!["d".into(), "my-article".into()]],
    ));
}

#[test]
fn matches_event_addresses_rejects_same_kind_different_pubkey() {
    let mut shape = InterestShape::default();
    shape.addresses.insert(NaddrCoord {
        pubkey: hex("aa"),
        kind: 30_023,
        d_tag: "my-article".to_string(),
    });

    // Same kind and same `d` tag, but a DIFFERENT author — over-delivery
    // that #3091 fixes: previously `addresses` was ignored so this matched
    // on kind alone.
    assert!(!shape.matches_event(
        &hex("bb"),
        30_023,
        100,
        &[vec!["d".into(), "my-article".into()]],
    ));
}

#[test]
fn matches_event_addresses_rejects_same_kind_and_author_different_d_tag() {
    let mut shape = InterestShape::default();
    shape.addresses.insert(NaddrCoord {
        pubkey: hex("aa"),
        kind: 30_023,
        d_tag: "my-article".to_string(),
    });

    // Same kind and author, but a DIFFERENT `d` tag — a different addressable
    // event entirely; must not match.
    assert!(!shape.matches_event(
        &hex("aa"),
        30_023,
        100,
        &[vec!["d".into(), "some-other-article".into()]],
    ));
    // No `d` tag at all also fails to match a non-empty-`d_tag` coordinate.
    assert!(!shape.matches_event(&hex("aa"), 30_023, 100, &[]));
}

#[test]
fn matches_event_addresses_empty_d_tag_matches_missing_or_empty_d_tag() {
    // Non-parameterized replaceable kind (e.g. kind:10002 NIP-65 relay list):
    // the coordinate's `d_tag` is empty, and the event itself never carries a
    // meaningful `d` tag.
    let mut shape = InterestShape::default();
    shape.addresses.insert(NaddrCoord {
        pubkey: hex("aa"),
        kind: 10_002,
        d_tag: String::new(),
    });

    // No `d` tag at all → matches.
    assert!(shape.matches_event(&hex("aa"), 10_002, 100, &[]));
    // An explicit but empty `d` tag → also matches.
    assert!(shape.matches_event(&hex("aa"), 10_002, 100, &[vec!["d".into(), String::new()]],));
    // A non-empty `d` tag on an empty-`d_tag` coordinate does NOT match.
    assert!(!shape.matches_event(
        &hex("aa"),
        10_002,
        100,
        &[vec!["d".into(), "unexpected".into()]],
    ));
}

#[test]
fn matches_event_addresses_is_or_across_multiple_coordinates() {
    let mut shape = InterestShape::default();
    shape.addresses.insert(NaddrCoord {
        pubkey: hex("aa"),
        kind: 30_023,
        d_tag: "article-1".to_string(),
    });
    shape.addresses.insert(NaddrCoord {
        pubkey: hex("bb"),
        kind: 30_023,
        d_tag: "article-2".to_string(),
    });

    // Matches the second coordinate only.
    assert!(shape.matches_event(
        &hex("bb"),
        30_023,
        100,
        &[vec!["d".into(), "article-2".into()]],
    ));
    // Matches neither.
    assert!(!shape.matches_event(
        &hex("cc"),
        30_023,
        100,
        &[vec!["d".into(), "article-3".into()]],
    ));
}
