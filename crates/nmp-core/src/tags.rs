//! Shared, kind-agnostic Nostr tag helpers + the NIP-10 reference parser.
//!
//! This module is the `getNip10References` / e-p-a-q tag-builder equivalent
//! from applesauce, refactored into NMP idiom. It lives in `nmp-core`
//! alongside [`crate::nip19`] and [`crate::nip21`] for the same reason those
//! do: it is a **protocol codec**, not a per-kind decoder or a domain noun.
//! D0 (`docs/design/kind-wrappers.md`) forbids the kernel knowing
//! "kind 30023 == article"; nothing here encodes any kind semantics — every
//! function is a pure transform over `&[Vec<String>]`. Per-kind decoders
//! (kind 7 → `ReactionRecord`, etc.) stay in their protocol crates.
//!
//! Both the per-NIP relation crates and the `nmp-relations` facade consume
//! these helpers so tag construction and NIP-10 interpretation are defined
//! exactly once.

use serde::{Deserialize, Serialize};

// ─── Tag constructors ────────────────────────────────────────────────────────

/// Build an `e` tag: `["e", <id>]`, optionally with a relay hint and a
/// NIP-10 marker (`"root"` / `"reply"` / `"mention"`).
///
/// NIP-10 marked form requires the relay slot to be present (possibly empty)
/// when a marker follows, so a `Some(marker)` always emits the 4-column form
/// `["e", id, relay_or_empty, marker]`.
pub fn e_tag(id: &str, relay: Option<&str>, marker: Option<&str>) -> Vec<String> {
    match (relay, marker) {
        (_, Some(marker)) => vec![
            "e".to_string(),
            id.to_string(),
            relay.unwrap_or("").to_string(),
            marker.to_string(),
        ],
        (Some(relay), None) => vec!["e".to_string(), id.to_string(), relay.to_string()],
        (None, None) => vec!["e".to_string(), id.to_string()],
    }
}

/// Build a `p` tag: `["p", <pubkey>]`, optionally with a relay hint.
pub fn p_tag(pubkey: &str, relay: Option<&str>) -> Vec<String> {
    match relay {
        Some(relay) => vec!["p".to_string(), pubkey.to_string(), relay.to_string()],
        None => vec!["p".to_string(), pubkey.to_string()],
    }
}

/// Build a NIP-33 `a` tag: `["a", "<kind>:<pubkey>:<d_tag>"]`, optionally with
/// a relay hint.
pub fn a_tag(kind: u32, pubkey: &str, d_tag: &str, relay: Option<&str>) -> Vec<String> {
    let coord = format!("{kind}:{pubkey}:{d_tag}");
    match relay {
        Some(relay) => vec!["a".to_string(), coord, relay.to_string()],
        None => vec!["a".to_string(), coord],
    }
}

/// Build a NIP-18 `q` (quote) tag: `["q", <id>]`, optionally with a relay hint.
pub fn q_tag(id: &str, relay: Option<&str>) -> Vec<String> {
    match relay {
        Some(relay) => vec!["q".to_string(), id.to_string(), relay.to_string()],
        None => vec!["q".to_string(), id.to_string()],
    }
}

// ─── Tag readers ─────────────────────────────────────────────────────────────

/// Return the second column of the first tag whose first column equals `key`.
///
/// Promoted here from the copy that was private to `nmp-nip23::decode` so
/// every protocol crate shares one implementation.
pub fn first_tag_value<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.first().map(String::as_str) == Some(key))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

/// Return the second column of every tag whose first column equals `key`,
/// in document order.
pub fn all_tag_values<'a>(tags: &'a [Vec<String>], key: &str) -> Vec<&'a str> {
    tags.iter()
        .filter(|t| t.first().map(String::as_str) == Some(key))
        .filter_map(|t| t.get(1))
        .map(String::as_str)
        .collect()
}

// ─── NIP-10 reference parser ─────────────────────────────────────────────────

/// A single `e`-tag reference: the pointed-to event id, plus the optional
/// relay hint and NIP-10 marker that accompanied it.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
}

/// The NIP-10 thread references decoded from an event's tags — the NMP
/// equivalent of applesauce's `getNip10References`.
///
/// `root` is the thread root, `reply` is the direct parent this event is
/// replying to (the `replyingTo$` target), `mentions` are quoted/mentioned
/// events, and `mentioned_pubkeys` carries the `p` tags so a reply builder
/// can re-notify the thread participants per NIP-10.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip10Refs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<EventRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<EventRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<EventRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentioned_pubkeys: Vec<String>,
}

impl Nip10Refs {
    /// True when the event carries no root and no reply marker — i.e. it is a
    /// thread root itself, not a reply (mirrors applesauce `Note.isRoot`).
    pub fn is_root(&self) -> bool {
        self.root.is_none() && self.reply.is_none()
    }

    /// True when the event replies to something (mirrors `Note.isReply`).
    pub fn is_reply(&self) -> bool {
        self.reply.is_some()
    }
}

fn e_ref_from_tag(tag: &[String]) -> Option<EventRef> {
    let id = tag.get(1)?.clone();
    if id.is_empty() {
        return None;
    }
    let relay = tag.get(2).filter(|s| !s.is_empty()).cloned();
    let marker = tag.get(3).filter(|s| !s.is_empty()).cloned();
    Some(EventRef { id, relay, marker })
}

/// Parse NIP-10 thread references from raw tags.
///
/// Supports the preferred **marked** form (`["e", id, relay, "root|reply|
/// mention"]`) and falls back to the deprecated **positional** convention
/// when no markers are present:
/// - 0 `e` tags → not a reply.
/// - 1 `e` tag → that event is both the root and the direct parent.
/// - ≥2 `e` tags → first is root, last is the direct parent, middle are
///   mentions.
///
/// When a `root` marker is present but no `reply` marker is, the reply target
/// is the root (a top-level reply to the thread root) — matching the common
/// client interpretation and applesauce's behaviour.
pub fn parse_nip10(tags: &[Vec<String>]) -> Nip10Refs {
    let e_tags: Vec<&Vec<String>> = tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("e"))
        .collect();

    let mentioned_pubkeys: Vec<String> = all_tag_values(tags, "p")
        .into_iter()
        .map(str::to_string)
        .collect();

    let has_marker = e_tags
        .iter()
        .any(|t| matches!(t.get(3).map(String::as_str), Some("root" | "reply" | "mention")));

    if has_marker {
        let mut refs = Nip10Refs {
            mentioned_pubkeys,
            ..Default::default()
        };
        for tag in &e_tags {
            let Some(eref) = e_ref_from_tag(tag) else {
                continue;
            };
            match eref.marker.as_deref() {
                Some("root") => {
                    if refs.root.is_none() {
                        refs.root = Some(eref);
                    }
                }
                Some("reply") => {
                    if refs.reply.is_none() {
                        refs.reply = Some(eref);
                    }
                }
                Some("mention") => refs.mentions.push(eref),
                _ => refs.mentions.push(eref),
            }
        }
        // Top-level reply to a root: a "root" with no explicit "reply".
        if refs.reply.is_none() {
            refs.reply = refs.root.clone();
        }
        return refs;
    }

    // Positional fallback (deprecated NIP-10 form).
    let resolved: Vec<EventRef> = e_tags
        .iter()
        .filter_map(|t| e_ref_from_tag(t))
        .collect();

    match resolved.len() {
        0 => Nip10Refs {
            mentioned_pubkeys,
            ..Default::default()
        },
        1 => Nip10Refs {
            root: Some(resolved[0].clone()),
            reply: Some(resolved[0].clone()),
            mentions: Vec::new(),
            mentioned_pubkeys,
        },
        n => Nip10Refs {
            root: Some(resolved[0].clone()),
            reply: Some(resolved[n - 1].clone()),
            mentions: resolved[1..n - 1].to_vec(),
            mentioned_pubkeys,
        },
    }
}

#[cfg(test)]
mod tests {
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
}
