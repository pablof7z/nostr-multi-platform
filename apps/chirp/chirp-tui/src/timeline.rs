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
    /// `true` only on the FIRST event of a `TimelineBlock::Module` whose
    /// `root` pointer is `Some` — i.e. the chain's top event is itself a
    /// reply to a missing ancestor (partial chain), NOT the true thread
    /// root. The left-pane post list uses this to render a "↳ reply in
    /// thread" indicator so partial-chain heads don't masquerade as roots.
    /// `depth` is intentionally left at `0` for the head event so the
    /// detail-pane navigation anchor (`depth == 0`) still works.
    pub is_partial_chain_head: bool,
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
                let (ids, has_gap, is_partial_chain) = ids_from_block(block);
                for (depth, id) in ids.into_iter().enumerate() {
                    if let Some(card) = cards.get(id.as_str()) {
                        // Flag belongs only to the chain's head event; the
                        // rest of the chain are ordinary replies.
                        let is_partial_chain_head = is_partial_chain && depth == 0;
                        rows.push(Self::from_card(card, depth, has_gap, is_partial_chain_head));
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

    fn from_card(card: &Value, depth: usize, has_gap: bool, is_partial_chain_head: bool) -> Self {
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
            is_partial_chain_head,
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

/// Extract event ids from a `TimelineBlock`, plus two structural flags
/// used downstream by the UI:
///
/// - `has_gap`: the module knows an ancestor / mid-chain event is missing.
/// - `is_partial_chain`: the module's Event root pointer names a different
///   id than the first event in the module. That means the displayed head is
///   a reply to a missing event ancestor. Non-Event roots terminate the chain
///   and do not imply a missing event head.
fn ids_from_block(block: &Value) -> (Vec<String>, bool, bool) {
    if let Some(id) = block.get("Standalone").and_then(Value::as_str) {
        return (vec![id.to_string()], false, false);
    }
    let Some(module) = block.get("Module") else {
        return (Vec::new(), false, false);
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
    let is_partial_chain = event_root_mismatches_top(module.get("root"), ids.first());
    (ids, has_gap, is_partial_chain)
}

fn event_root_mismatches_top(root: Option<&Value>, top: Option<&String>) -> bool {
    let Some(top) = top else {
        return false;
    };
    root.and_then(|root| root.get("Event"))
        .and_then(|event| event.get("id"))
        .and_then(Value::as_str)
        .is_some_and(|root_id| root_id != top)
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
#[path = "timeline/tests.rs"]
mod tests;
