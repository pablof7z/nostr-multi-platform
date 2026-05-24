use nmp_core::display::short_npub;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineRow {
    pub id: String,
    pub author: String,
    pub author_pubkey: String,
    pub content: String,
    pub created_at: u64,
    pub depth: usize,
    pub has_gap: bool,
    pub relation_counts: RowRelationCounts,
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

        if rows.is_empty() {
            rows.extend(cards.values().map(|card| Self::from_card(card, 0, false)));
            rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        }

        rows
    }

    fn from_card(card: &Value, depth: usize, has_gap: bool) -> Self {
        let id = string_field(card, "id");
        let author_pubkey = string_field(card, "author_pubkey");
        let author = card
            .get("author_display")
            .and_then(|display| display.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| short_npub(&author_pubkey));
        let content = string_field(card, "content");
        let created_at = card.get("created_at").and_then(Value::as_u64).unwrap_or(0);
        Self {
            id,
            author,
            author_pubkey,
            content: content_preview(&content),
            created_at,
            depth,
            has_gap,
            relation_counts: RowRelationCounts::from_card(card),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowRelationCounts {
    pub replies: RowRelationCount,
    pub reactions: RowRelationCount,
    pub reposts: RowRelationCount,
}

impl RowRelationCounts {
    fn from_card(card: &Value) -> Self {
        let relation_counts = card.get("relation_counts");
        Self {
            replies: count_from(relation_counts, "replies"),
            reactions: count_from(relation_counts, "reactions"),
            reposts: count_from(relation_counts, "reposts"),
        }
    }

    #[must_use] 
    pub fn summary(&self) -> String {
        format!(
            "reply {}  react {}  repost {}",
            self.replies.label(),
            self.reactions.label(),
            self.reposts.label()
        )
    }
}

impl Default for RowRelationCounts {
    fn default() -> Self {
        Self {
            replies: RowRelationCount::Loading,
            reactions: RowRelationCount::Loading,
            reposts: RowRelationCount::Loading,
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


fn content_preview(content: &str) -> String {
    let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= 96 {
        compact
    } else {
        let preview = compact.chars().take(95).collect::<String>();
        format!("{preview}...")
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
            "cards": [{
                "id": "note",
                "author_pubkey": "aaaaaaaaaaaaaaaa",
                "author_display": {"name": "Alice"},
                "created_at": 1,
                "content": "hello",
                "relation_counts": {
                    "replies": {"state": "known", "count": 2},
                    "reactions": {"state": "known", "count": 3},
                    "reposts": {"state": "known", "count": 1}
                }
            }]
        });

        let rows = TimelineRow::from_snapshot(&snapshot);

        assert_eq!(rows[0].author, "Alice");
        assert_eq!(rows[0].relation_counts.replies, RowRelationCount::Known(2));
        assert_eq!(
            rows[0].relation_counts.reactions,
            RowRelationCount::Known(3)
        );
        assert_eq!(rows[0].relation_counts.reposts, RowRelationCount::Known(1));
    }

    #[test]
    fn relation_counts_preserve_loading_vs_known_zero() {
        let snapshot = serde_json::json!({
            "cards": [{
                "id": "note",
                "author_pubkey": "aaaaaaaaaaaaaaaa",
                "created_at": 1,
                "content": "hello",
                "relation_counts": {
                    "replies": {"state": "known", "count": 0},
                    "reactions": {"state": "loading", "interest": {"namespace": "nmp.reactions.summary"}},
                    "reposts": {"state": "known", "count": 0}
                }
            }]
        });

        let rows = TimelineRow::from_snapshot(&snapshot);

        assert_eq!(rows[0].relation_counts.replies, RowRelationCount::Known(0));
        assert_eq!(rows[0].relation_counts.reactions, RowRelationCount::Loading);
        assert_eq!(rows[0].relation_counts.reposts, RowRelationCount::Known(0));
        assert_eq!(
            rows[0].relation_counts.summary(),
            "reply 0  react ...  repost 0"
        );
    }
}
