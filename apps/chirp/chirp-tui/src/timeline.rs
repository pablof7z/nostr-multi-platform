use nmp_core::display::{short_npub, to_npub};
use serde_json::Value;

use crate::ui::nostr_content::{
    content_render_data::ContentRenderData, content_tree_wire::ContentTreeWire,
};
use crate::ui::nostr_user::profile_wire::ProfileWire;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineRow {
    pub id: String,
    pub author_pubkey: String,
    pub author_profile: ProfileWire,
    pub content: String,
    /// Timestamp shown next to the author label. For ordinary notes this is
    /// the note's own `created_at`; for repost rows it's the *original*
    /// note's publish time (the repost timestamp lives on `repost`).
    pub created_at: u64,
    pub depth: usize,
    pub has_gap: bool,
    pub relation_counts: RowRelationCounts,
    pub content_tree: Option<ContentTreeWire>,
    pub content_render: ContentRenderData,
    /// 64-hex pubkeys appearing as NIP-21 profile mentions inside this
    /// row's `content_tree`. Sorted + deduped at construction so a stable
    /// equality holds across snapshot ticks (`RenderIntentTracker` diff-set
    /// math relies on stable orderings to avoid spurious claim churn).
    pub mention_pubkeys: Vec<String>,
    /// `Some` when this row surfaced because of a NIP-18 repost — the
    /// `author_*` fields above name the original note's author; this struct
    /// names the reposter so the UI can show "↻ reposted by @<reposter>".
    pub repost: Option<RowRepost>,
    /// Pretty-printed JSON of the raw card object from the NMP snapshot.
    pub raw_card: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowRepost {
    pub author_pubkey: String,
    pub author_profile: ProfileWire,
    pub repost_created_at: u64,
}

impl TimelineRow {
    pub fn from_snapshot(snapshot: &Value) -> Vec<Self> {
        let cards = snapshot
            .get("cards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|card| {
                let id = card.get("id")?.as_str()?.to_string();
                Some((id, card))
            })
            .collect::<std::collections::HashMap<_, _>>();

        let mut rows = Vec::new();
        if let Some(blocks) = snapshot.get("blocks").and_then(Value::as_array) {
            for block in blocks {
                let (ids, has_gap) = ids_from_block(block);
                for (depth, id) in ids.into_iter().enumerate() {
                    if let Some(card) = cards.get(id.as_str()) {
                        rows.push(Self::from_card(card, depth, has_gap));
                    }
                }
            }
        }

        rows
    }

    pub fn author_label(&self) -> &str {
        self.author_profile.display()
    }

    pub fn media_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        if let Some(tree) = self.content_tree.as_ref() {
            push_unique_urls(&mut urls, tree.media_urls());
        }
        push_unique_urls(&mut urls, self.content_render.media_urls());
        urls
    }

    fn from_card(card: &Value, depth: usize, has_gap: bool) -> Self {
        let id = string_field(card, "id");
        let author_pubkey = string_field(card, "author_pubkey");
        let author_profile = author_profile_from_card(&author_pubkey, card);
        let content = string_field(card, "content");
        let outer_created_at = card.get("created_at").and_then(Value::as_u64).unwrap_or(0);
        let content_tree = card
            .get("content_tree")
            .and_then(ContentTreeWire::from_value);
        let mention_pubkeys = content_tree
            .as_ref()
            .map(ContentTreeWire::mentioned_pubkeys)
            .unwrap_or_default();
        let content_render = ContentRenderData::from_value(card.get("content_render"));
        let repost = repost_from_card(card.get("reposted_by"), outer_created_at);
        // For reposts the displayed timestamp is the original note's publish
        // time (carried on `reposted_by.note_created_at`); the card's own
        // `created_at` is the repost timestamp (used as the feed sort key
        // and shown on the attribution line).
        let created_at = repost
            .as_ref()
            .map(|_| {
                card.get("reposted_by")
                    .and_then(|r| r.get("note_created_at"))
                    .and_then(Value::as_u64)
                    .unwrap_or(outer_created_at)
            })
            .unwrap_or(outer_created_at);
        Self {
            id,
            author_pubkey,
            author_profile,
            content,
            created_at,
            depth,
            has_gap,
            relation_counts: RowRelationCounts::from_card(card),
            content_tree,
            content_render,
            mention_pubkeys,
            repost,
            raw_card: serde_json::to_string_pretty(card).unwrap_or_default(),
        }
    }
}

fn repost_from_card(value: Option<&Value>, outer_created_at: u64) -> Option<RowRepost> {
    let attribution = value?;
    let author_pubkey = attribution.get("author_pubkey")?.as_str()?.to_string();
    let author_profile = author_profile_from_card(&author_pubkey, attribution);
    Some(RowRepost {
        author_pubkey,
        author_profile,
        repost_created_at: outer_created_at,
    })
}

fn author_profile_from_card(pubkey: &str, card: &Value) -> ProfileWire {
    let display = card.get("author_display");
    ProfileWire {
        pubkey: pubkey.to_string(),
        display_name: optional_string(display, "name")
            .or_else(|| optional_string(Some(card), "author_display_name")),
        about: optional_string(display, "about"),
        picture_url: optional_string(display, "picture_url")
            .or_else(|| optional_string(Some(card), "author_picture_url")),
        nip05: optional_string(display, "nip05"),
        npub: optional_string(display, "npub")
            .or_else(|| optional_string(Some(card), "author_npub"))
            .unwrap_or_else(|| to_npub(pubkey)),
        npub_short: short_npub(pubkey),
    }
}

fn push_unique_urls(out: &mut Vec<String>, urls: Vec<String>) {
    for url in urls {
        if !out.iter().any(|existing| existing == &url) {
            out.push(url);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowRelationCounts {
    pub replies: RowRelationCount,
    pub reactions: RowRelationCount,
    pub reposts: RowRelationCount,
    pub zaps: RowRelationCount,
}

impl RowRelationCounts {
    fn from_card(card: &Value) -> Self {
        let relation_counts = card.get("relation_counts");
        Self {
            replies: count_from(relation_counts, "replies"),
            reactions: count_from(relation_counts, "reactions"),
            reposts: count_from(relation_counts, "reposts"),
            zaps: count_from(relation_counts, "zaps"),
        }
    }

    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "reply {}  react {}  repost {}  zap {}",
            self.replies.label(),
            self.reactions.label(),
            self.reposts.label(),
            self.zaps.label()
        )
    }
}

impl Default for RowRelationCounts {
    fn default() -> Self {
        Self {
            replies: RowRelationCount::Loading,
            reactions: RowRelationCount::Loading,
            reposts: RowRelationCount::Loading,
            zaps: RowRelationCount::Loading,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowRelationCount {
    Known(u64),
    Loading,
}

impl RowRelationCount {
    fn label(&self) -> String {
        match self {
            Self::Known(count) => count.to_string(),
            Self::Loading => "...".to_string(),
        }
    }
}

fn ids_from_block(block: &Value) -> (Vec<String>, bool) {
    if let Some(id) = block.get("Standalone").and_then(Value::as_str) {
        return (vec![id.to_string()], false);
    }
    let Some(module) = block.get("Module") else {
        return (Vec::new(), false);
    };
    let ids = module
        .get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let has_gap = module
        .get("has_gap")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (ids, has_gap)
}

fn string_field(card: &Value, key: &str) -> String {
    card.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn optional_string(value: Option<&Value>, key: &str) -> Option<String> {
    value
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn count_from(relation_counts: Option<&Value>, key: &str) -> RowRelationCount {
    let Some(value) = relation_counts.and_then(|counts| counts.get(key)) else {
        return RowRelationCount::Loading;
    };
    match value.get("state").and_then(Value::as_str) {
        Some("known") => value
            .get("count")
            .and_then(Value::as_u64)
            .map_or(RowRelationCount::Loading, RowRelationCount::Known),
        _ => RowRelationCount::Loading,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_rows_follow_block_order() {
        let snapshot = serde_json::json!({
            "blocks": [
                {"Module": {"events": ["root", "reply"], "has_gap": true, "root": null}},
                {"Standalone": "solo"}
            ],
            "cards": [
                {"id": "solo", "author_pubkey": "bbbbbbbbbbbbbbbb", "kind": 1, "created_at": 3, "content": "solo note"},
                {"id": "reply", "author_pubkey": "cccccccccccccccc", "kind": 1, "created_at": 2, "content": "reply note"},
                {"id": "root", "author_pubkey": "aaaaaaaaaaaaaaaa", "kind": 1, "created_at": 1, "content": "root note"}
            ]
        });

        let rows = TimelineRow::from_snapshot(&snapshot);

        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec!["root", "reply", "solo"]
        );
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].depth, 1);
        assert!(rows[1].has_gap);
    }

    #[test]
    fn row_uses_profile_display_and_relation_counts_when_present() {
        let snapshot = serde_json::json!({
            "blocks": [{"Standalone": "note"}],
            "cards": [{
                "id": "note",
                "author_pubkey": "aaaaaaaaaaaaaaaa",
                "author_display": {"name": "Alice"},
                "created_at": 1,
                "content": "hello",
                "relation_counts": {
                    "replies": {"state": "known", "count": 2},
                    "reactions": {"state": "known", "count": 3},
                    "reposts": {"state": "known", "count": 1},
                    "zaps": {"state": "known", "count": 4}
                }
            }]
        });

        let rows = TimelineRow::from_snapshot(&snapshot);

        assert_eq!(rows[0].author_label(), "Alice");
        assert_eq!(
            rows[0].author_profile.display_name.as_deref(),
            Some("Alice")
        );
        assert_eq!(rows[0].relation_counts.replies, RowRelationCount::Known(2));
        assert_eq!(
            rows[0].relation_counts.reactions,
            RowRelationCount::Known(3)
        );
        assert_eq!(rows[0].relation_counts.reposts, RowRelationCount::Known(1));
        assert_eq!(rows[0].relation_counts.zaps, RowRelationCount::Known(4));
    }

    #[test]
    fn mention_pubkeys_extracted_from_content_tree() {
        let mention_a = "a".repeat(64);
        let mention_b = "b".repeat(64);
        let snapshot = serde_json::json!({
            "blocks": [{"Standalone": "note"}],
            "cards": [{
                "id": "note",
                "author_pubkey": "aaaaaaaaaaaaaaaa",
                "created_at": 1,
                "content": "hello",
                "content_tree": {
                    "nodes": [
                        {"kind": "text", "text": "hi "},
                        {
                            "kind": "mention",
                            "uri": {
                                "uri": "nostr:npub1...",
                                "kind": "profile",
                                "primary_id": mention_a,
                                "relays": [],
                            }
                        },
                        {"kind": "text", "text": " and "},
                        {
                            "kind": "mention",
                            "uri": {
                                "uri": "nostr:npub1...",
                                "kind": "profile",
                                "primary_id": mention_b,
                                "relays": [],
                            }
                        },
                    ],
                    "roots": [0, 1, 2, 3],
                    "mode": "plaintext"
                }
            }]
        });
        let rows = TimelineRow::from_snapshot(&snapshot);
        assert_eq!(rows[0].mention_pubkeys, vec![mention_a, mention_b]);
    }

    #[test]
    fn mention_pubkeys_filter_non_hex_and_short_ids() {
        let snapshot = serde_json::json!({
            "blocks": [{"Standalone": "note"}],
            "cards": [{
                "id": "note",
                "author_pubkey": "aaaaaaaaaaaaaaaa",
                "created_at": 1,
                "content": "hello",
                "content_tree": {
                    "nodes": [
                        {
                            "kind": "mention",
                            "uri": {
                                "uri": "nostr:npub1...",
                                "kind": "profile",
                                "primary_id": "too-short",
                                "relays": [],
                            }
                        },
                        {
                            "kind": "mention",
                            "uri": {
                                "uri": "nostr:npub1...",
                                "kind": "profile",
                                // 64 chars but with a non-hex `z` mid-string.
                                "primary_id": "zzzz1111111111111111111111111111111111111111111111111111111111zz",
                                "relays": [],
                            }
                        },
                    ],
                    "roots": [0, 1],
                    "mode": "plaintext"
                }
            }]
        });
        let rows = TimelineRow::from_snapshot(&snapshot);
        assert!(
            rows[0].mention_pubkeys.is_empty(),
            "non-hex / wrong-length mention ids must be filtered, got {:?}",
            rows[0].mention_pubkeys
        );
    }

    #[test]
    fn mention_pubkeys_dedup_and_sort() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let snapshot = serde_json::json!({
            "blocks": [{"Standalone": "note"}],
            "cards": [{
                "id": "note",
                "author_pubkey": "x",
                "created_at": 1,
                "content": "",
                "content_tree": {
                    "nodes": [
                        // Duplicate mention should collapse to one entry.
                        {"kind": "mention", "uri": {"uri": "", "kind": "profile", "primary_id": b, "relays": []}},
                        {"kind": "mention", "uri": {"uri": "", "kind": "profile", "primary_id": a, "relays": []}},
                        {"kind": "mention", "uri": {"uri": "", "kind": "profile", "primary_id": b, "relays": []}},
                    ],
                    "roots": [0, 1, 2],
                    "mode": "plaintext"
                }
            }]
        });
        let rows = TimelineRow::from_snapshot(&snapshot);
        assert_eq!(rows[0].mention_pubkeys, vec![a, b]);
    }

    #[test]
    fn missing_content_tree_yields_empty_mention_pubkeys() {
        let snapshot = serde_json::json!({
            "blocks": [{"Standalone": "note"}],
            "cards": [{
                "id": "note",
                "author_pubkey": "x",
                "created_at": 1,
                "content": "hello",
            }]
        });
        let rows = TimelineRow::from_snapshot(&snapshot);
        assert!(rows[0].mention_pubkeys.is_empty());
    }

    #[test]
    fn media_urls_include_direct_and_quoted_event_media() {
        let snapshot = serde_json::json!({
            "blocks": [{"Standalone": "note"}],
            "cards": [{
                "id": "note",
                "author_pubkey": "x",
                "created_at": 1,
                "content": "media note",
                "content_tree": {
                    "nodes": [{
                        "kind": "media",
                        "media_kind": "image",
                        "urls": ["https://example.com/direct.jpg"],
                    }],
                    "roots": [0],
                    "mode": "plaintext"
                },
                "content_render": {
                    "events": {
                        "quoted": {
                            "id": "quoted",
                            "author_pubkey": "y",
                            "content_tree": {
                                "nodes": [{
                                    "kind": "image",
                                    "alt": "",
                                    "src": "https://example.com/quote.webp",
                                }],
                                "roots": [0],
                                "mode": "plaintext"
                            }
                        }
                    }
                }
            }]
        });
        let rows = TimelineRow::from_snapshot(&snapshot);
        assert_eq!(
            rows[0].media_urls(),
            vec![
                "https://example.com/direct.jpg".to_string(),
                "https://example.com/quote.webp".to_string(),
            ]
        );
    }

    #[test]
    fn repost_card_uses_inner_timestamp_and_attaches_repost_attribution() {
        // The card represents the original note (kind:1 author, kind:1 content)
        // but its outer `created_at` is the kind:6 repost time. The row's
        // displayed `created_at` should be the inner note's publish time;
        // `repost` carries the reposter + repost timestamp for the "↻ reposted
        // by" line.
        let snapshot = serde_json::json!({
            "blocks": [{"Standalone": "repost"}],
            "cards": [{
                "id": "repost",
                "author_pubkey": "innerinnerinnerinnerinnerinnerinnerinnerinnerinnerinnerinner1234",
                "author_display": {"name": "calle"},
                "created_at": 100,
                "content": "Imagine BlueSky but with Nutzaps",
                "reposted_by": {
                    "author_pubkey": "reposterreposterreposterreposterreposterreposterreposterreposte",
                    "author_display": {"name": "pablof7z"},
                    "note_created_at": 50,
                }
            }]
        });

        let rows = TimelineRow::from_snapshot(&snapshot);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].author_label(), "calle");
        assert_eq!(rows[0].created_at, 50, "displayed time is the original note's");
        let repost = rows[0].repost.as_ref().expect("repost attribution present");
        assert_eq!(
            repost.author_pubkey,
            "reposterreposterreposterreposterreposterreposterreposterreposte"
        );
        assert_eq!(repost.author_profile.display_name.as_deref(), Some("pablof7z"));
        assert_eq!(
            repost.repost_created_at, 100,
            "repost line shows the kind:6 timestamp"
        );
    }

    #[test]
    fn ordinary_note_has_no_repost_attribution() {
        let snapshot = serde_json::json!({
            "blocks": [{"Standalone": "note"}],
            "cards": [{
                "id": "note",
                "author_pubkey": "aaaaaaaaaaaaaaaa",
                "created_at": 1,
                "content": "hello"
            }]
        });
        let rows = TimelineRow::from_snapshot(&snapshot);
        assert!(rows[0].repost.is_none());
    }

    #[test]
    fn relation_counts_preserve_loading_vs_known_zero() {
        let snapshot = serde_json::json!({
            "blocks": [{"Standalone": "note"}],
            "cards": [{
                "id": "note",
                "author_pubkey": "aaaaaaaaaaaaaaaa",
                "created_at": 1,
                "content": "hello",
                "relation_counts": {
                    "replies": {"state": "known", "count": 0},
                    "reactions": {"state": "loading", "interest": {"namespace": "nmp.reactions.summary"}},
                    "reposts": {"state": "known", "count": 0},
                    "zaps": {"state": "known", "count": 0}
                }
            }]
        });

        let rows = TimelineRow::from_snapshot(&snapshot);

        assert_eq!(rows[0].relation_counts.replies, RowRelationCount::Known(0));
        assert_eq!(rows[0].relation_counts.reactions, RowRelationCount::Loading);
        assert_eq!(rows[0].relation_counts.reposts, RowRelationCount::Known(0));
        assert_eq!(
            rows[0].relation_counts.summary(),
            "reply 0  react ...  repost 0  zap 0"
        );
    }

    /// Regression: when `blocks` is present but its IDs temporarily don't
    /// match any card (blocks/cards desync mid-session), replies must NOT be
    /// promoted to depth 0.  The correct result is an empty row list.
    #[test]
    fn blocks_present_but_ids_missing_from_cards_yields_empty() {
        let snapshot = serde_json::json!({
            "blocks": [
                {"Module": {"events": ["root", "reply"], "has_gap": false, "root": null}}
            ],
            "cards": [
                {"id": "other", "author_pubkey": "aaaaaaaaaaaaaaaa", "created_at": 1, "content": "reply text"}
            ]
        });

        let rows = TimelineRow::from_snapshot(&snapshot);

        assert!(
            rows.is_empty(),
            "blocks present but no matching cards → empty rows; got {:?}",
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_blocks_key_yields_empty_rows() {
        let snapshot = serde_json::json!({
            "cards": [
                {"id": "a", "author_pubkey": "aaaaaaaaaaaaaaaa", "created_at": 2, "content": "root"},
                {"id": "b", "author_pubkey": "bbbbbbbbbbbbbbbb", "created_at": 1, "content": "reply"}
            ]
        });

        let rows = TimelineRow::from_snapshot(&snapshot);

        assert!(rows.is_empty());
    }
}
