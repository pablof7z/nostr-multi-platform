//! Unit tests for the shared tag constructors / readers / NIP-10 parser and
//! the [`super::capped_contact_follows`] follow-cap function.
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

// ── follow cap (capped_contact_follows) ─────────────────────────────────

/// Deterministic distinct valid 64-hex pubkey for index `i`.
fn hex_pk(i: usize) -> String {
    format!(
        "{:016x}{}",
        i as u64,
        "0123456789abcdef0123456789abcdef0123456789abcdef"
    )
}

fn p_tags(follows: &[String]) -> Vec<Vec<String>> {
    follows
        .iter()
        .map(|pk| vec!["p".to_string(), pk.clone()])
        .collect()
}

#[test]
fn capped_follows_keeps_first_500_in_order() {
    let follows: Vec<String> = (0..600).map(hex_pk).collect();
    let capped = capped_contact_follows(&p_tags(&follows));
    assert_eq!(capped.len(), TIMELINE_AUTHOR_LIMIT);
    assert_eq!(capped, follows[..TIMELINE_AUTHOR_LIMIT].to_vec());
}

#[test]
fn capped_follows_below_cap_returns_all() {
    let follows: Vec<String> = (0..3).map(hex_pk).collect();
    assert_eq!(capped_contact_follows(&p_tags(&follows)), follows);
}

#[test]
fn capped_follows_skips_non_hex_p_values() {
    // Malformed `p` values (too short, non-hex) are skipped — matching the
    // kernel's `is_hex_pubkey` filter — and do NOT occupy a cap slot.
    let valid_a = hex_pk(1);
    let valid_b = hex_pk(2);
    let tags = vec![
        vec!["p".to_string(), "not-hex".to_string()],
        vec!["p".to_string(), valid_a.clone()],
        vec!["p".to_string(), "tooshort".to_string()],
        vec!["p".to_string(), valid_b.clone()],
    ];
    assert_eq!(capped_contact_follows(&tags), vec![valid_a, valid_b]);
}

#[test]
fn capped_follows_ignores_non_p_tags() {
    let pk = hex_pk(7);
    let tags = vec![
        vec!["e".to_string(), hex_pk(99)],
        vec!["p".to_string(), pk.clone()],
        vec!["t".to_string(), "topic".to_string()],
    ];
    assert_eq!(capped_contact_follows(&tags), vec![pk]);
}

#[test]
fn capped_follows_preserves_duplicate_slots_no_dedup() {
    // The kernel collects into a `Vec` (no dedup), so a duplicate `p` tag
    // occupies a cap slot exactly as it does on the wire. The function must
    // mirror that — dedup would silently change which authors fall inside the
    // cap when an account has near-500 follows with duplicates.
    let pk = hex_pk(3);
    let tags = vec![
        vec!["p".to_string(), pk.clone()],
        vec!["p".to_string(), pk.clone()],
    ];
    assert_eq!(capped_contact_follows(&tags), vec![pk.clone(), pk]);
}

// ── NIP-10 marked form ──────────────────────────────────────────────────

#[test]
fn marked_root_and_reply() {
    let tags = vec![
        e_tag("ROOT", Some("wss://a"), Some("root")),
        e_tag("PARENT", Some("wss://b"), Some("reply")),
        vec!["p".into(), "author".into()],
    ];
    let r = parse_nip10(&tags);
    assert_eq!(r.root.as_ref().unwrap().id, "ROOT");
    assert_eq!(r.root.as_ref().unwrap().relay.as_deref(), Some("wss://a"));
    assert_eq!(r.reply.as_ref().unwrap().id, "PARENT");
    assert!(r.is_reply());
    assert!(!r.is_root());
    assert_eq!(r.mentioned_pubkeys, vec!["author"]);
}

#[test]
fn marked_root_only_makes_reply_equal_root() {
    let tags = vec![e_tag("ROOT", None, Some("root"))];
    let r = parse_nip10(&tags);
    assert_eq!(r.root.as_ref().unwrap().id, "ROOT");
    assert_eq!(r.reply.as_ref().unwrap().id, "ROOT");
}

#[test]
fn marked_mention_collected_separately() {
    let tags = vec![
        e_tag("ROOT", None, Some("root")),
        e_tag("PARENT", None, Some("reply")),
        e_tag("QUOTED", None, Some("mention")),
    ];
    let r = parse_nip10(&tags);
    assert_eq!(r.mentions.len(), 1);
    assert_eq!(r.mentions[0].id, "QUOTED");
}

// ── NIP-10 positional fallback ──────────────────────────────────────────

#[test]
fn positional_zero_e_tags_is_root_note() {
    let r = parse_nip10(&[vec!["p".into(), "x".into()]]);
    assert!(r.is_root());
    assert!(!r.is_reply());
}

#[test]
fn positional_single_e_tag_is_root_and_reply() {
    let r = parse_nip10(&[vec!["e".into(), "ONLY".into()]]);
    assert_eq!(r.root.as_ref().unwrap().id, "ONLY");
    assert_eq!(r.reply.as_ref().unwrap().id, "ONLY");
    assert!(r.mentions.is_empty());
}

#[test]
fn positional_two_e_tags_first_root_last_reply() {
    let r = parse_nip10(&[
        vec!["e".into(), "ROOT".into()],
        vec!["e".into(), "PARENT".into()],
    ]);
    assert_eq!(r.root.as_ref().unwrap().id, "ROOT");
    assert_eq!(r.reply.as_ref().unwrap().id, "PARENT");
    assert!(r.mentions.is_empty());
}

#[test]
fn positional_three_e_tags_middle_is_mention() {
    let r = parse_nip10(&[
        vec!["e".into(), "ROOT".into()],
        vec!["e".into(), "MID".into()],
        vec!["e".into(), "PARENT".into()],
    ]);
    assert_eq!(r.root.as_ref().unwrap().id, "ROOT");
    assert_eq!(r.reply.as_ref().unwrap().id, "PARENT");
    assert_eq!(r.mentions.len(), 1);
    assert_eq!(r.mentions[0].id, "MID");
}

#[test]
fn empty_e_tag_id_is_ignored() {
    let r = parse_nip10(&[vec!["e".into(), "".into()]]);
    assert!(r.is_root());
}

#[test]
fn nip10refs_json_roundtrips_and_skips_empty() {
    let refs = Nip10Refs {
        root: Some(EventRef {
            id: "ROOT".into(),
            relay: None,
            marker: Some("root".into()),
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&refs).unwrap();
    assert!(!json.contains("mentions"));
    assert!(!json.contains("\"relay\""));
    let back: Nip10Refs = serde_json::from_str(&json).unwrap();
    assert_eq!(back, refs);
}
