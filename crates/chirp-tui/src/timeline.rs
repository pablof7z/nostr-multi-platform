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
        let content = string_field(card, "content");
        let created_at = card.get("created_at").and_then(Value::as_u64).unwrap_or(0);
        Self {
            id,
            author: short_key(&author_pubkey),
            author_pubkey,
            content: content_preview(&content),
            created_at,
            depth,
            has_gap,
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

fn short_key(value: &str) -> String {
    if value.len() <= 12 {
        return value.to_string();
    }
    format!("{}...{}", &value[..8], &value[value.len() - 4..])
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
}
