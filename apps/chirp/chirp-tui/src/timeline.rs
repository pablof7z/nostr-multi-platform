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
    /// Raw attribution list (V-80 OP-centric feed): the follows whose replies
    /// referenced this root. Carried RAW — pubkey, display name mirror, reply
    /// id, reply timestamp — newest-last as the engine ordered them. The TUI
    /// renders only the most-recent 1 (Q1 display decision); the projection
    /// carries all of them so iOS may render N. Empty for roots no follow
    /// replied to.
    pub thread_attribution: Vec<RowReplyAttribution>,
    pub relation_counts: RowRelationCounts,
    pub relay_provenance: Vec<String>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowRepost {
    pub author_pubkey: String,
    pub author_profile: ProfileWire,
    pub repost_created_at: u64,
}

/// One follow's reply attributed to a feed root (V-80 OP-centric feed).
///
/// Raw mirror of `nmp_nip01::op_feed::Nip10ReplyAttribution`: the replying
/// follow's pubkey, the kind:0 display-name mirror (`None` until a kind:0
/// arrives — the renderer formats the raw pubkey as fallback), the reply event
/// id, and the reply timestamp. The TUI renders only the most-recent replier;
/// the full list is preserved so other surfaces can render more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowReplyAttribution {
    pub author_pubkey: String,
    pub author_profile: ProfileWire,
    pub reply_event_id: String,
    pub reply_created_at: u64,
}

impl TimelineRow {
    /// Parse a `RootFeedSnapshot` (`{ "cards": [{ "card": …, "attribution": […]
    /// }], "page": …, "metrics": … }`) into render rows.
    ///
    /// Each entry is one feed root — the home feed is thread-roots-only (V-80):
    /// replies never appear as standalone rows. The inner `card` is a
    /// `TimelineEventCard`; for reposts its `id` is the superseded target id
    /// (the engine keys repost slots by `target_id`, so the inline card already
    /// carries the right identity — no separate cards-map lookup is needed). The
    /// `attribution` array carries the follows whose replies surfaced or
    /// referenced this root.
    pub fn from_snapshot(snapshot: &Value) -> Vec<Self> {
        snapshot
            .get("cards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let card = entry.get("card")?;
                let attribution = attribution_from_entry(entry.get("attribution"));
                Some(Self::from_card(card, attribution))
            })
            .collect()
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

    fn from_card(card: &Value, thread_attribution: Vec<RowReplyAttribution>) -> Self {
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
        let relay_provenance = string_array_field(card, "relay_provenance");
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
            // Every feed entry is a thread root: depth 0, no chain gap. The
            // OP-centric feed never surfaces replies as their own rows, so the
            // partial-chain / multi-depth machinery is gone.
            depth: 0,
            has_gap: false,
            thread_attribution,
            relation_counts: RowRelationCounts::from_card(card),
            relay_provenance,
            content_tree,
            content_render,
            mention_pubkeys,
            repost,
        }
    }

    #[must_use]
    pub fn note_details_text(&self) -> String {
        let mut lines = vec![
            format!("id {}", self.id),
            format!("author {}", self.author_label()),
            format!("pubkey {}", self.author_pubkey),
            format!("created_at {}", self.created_at),
            self.relation_counts.summary(),
        ];
        if let Some(repost) = &self.repost {
            lines.push(format!(
                "reposted_by {} at {}",
                repost.author_profile.display(),
                repost.repost_created_at
            ));
        }
        if !self.thread_attribution.is_empty() {
            lines.push(format!(
                "reply_attribution {} follow replies",
                self.thread_attribution.len()
            ));
        }
        if !self.relay_provenance.is_empty() {
            lines.push(format!(
                "received_from {} relays",
                self.relay_provenance.len()
            ));
            lines.extend(
                self.relay_provenance
                    .iter()
                    .map(|relay| format!("  {relay}")),
            );
        }
        if !self.mention_pubkeys.is_empty() {
            lines.push(format!("mentions {} profiles", self.mention_pubkeys.len()));
        }
        let media_count = self.media_urls().len();
        if media_count > 0 {
            lines.push(format!("media {} urls", media_count));
        }
        lines.push(String::new());
        lines.push("content".to_string());
        if self.content.is_empty() {
            lines.push("(empty)".to_string());
        } else {
            lines.push(self.content.clone());
        }
        lines.join("\n")
    }
}

/// Parse the `attribution` array of a `RootCard` into raw render attributions.
///
/// Each element mirrors `Nip10ReplyAttribution` (`author_pubkey`,
/// `author_display`, `author_display_name`, `author_picture_url`,
/// `reply_event_id`, `reply_created_at`). Display fields fall back exactly as
/// the card author does (`author_profile_from_card`): the nested
/// `author_display` object first, the flat `author_display_name` /
/// `author_picture_url` mirrors next, the raw pubkey last.
fn attribution_from_entry(value: Option<&Value>) -> Vec<RowReplyAttribution> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let author_pubkey = item.get("author_pubkey")?.as_str()?.to_string();
            let author_profile = author_profile_from_card(&author_pubkey, item);
            Some(RowReplyAttribution {
                author_pubkey,
                author_profile,
                reply_event_id: string_field(item, "reply_event_id"),
                reply_created_at: item
                    .get("reply_created_at")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            })
        })
        .collect()
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

fn string_field(card: &Value, key: &str) -> String {
    card.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_array_field(card: &Value, key: &str) -> Vec<String> {
    card.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
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
