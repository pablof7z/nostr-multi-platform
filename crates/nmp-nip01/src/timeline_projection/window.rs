use std::{cmp::Ordering, collections::BTreeSet};

use nmp_content::{WireNode, WireNostrUriKind};
use nmp_core::substrate::BoundedMessageMap;
use nmp_threading::TimelineBlock;
use serde::{Deserialize, Serialize};

use super::TimelineEventCard;

pub const DEFAULT_TIMELINE_WINDOW_LIMIT: usize = 80;
pub const MAX_TIMELINE_WINDOW_LIMIT: usize = 500;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimelineWindowCursor {
    pub created_at: u64,
    pub id: String,
}

impl TimelineWindowCursor {
    #[must_use]
    pub(crate) fn is_newer_than(&self, other: &Self) -> bool {
        self.created_at > other.created_at
            || (self.created_at == other.created_at && self.id > other.id)
    }

    #[must_use]
    pub(crate) fn is_older_than(&self, other: &Self) -> bool {
        self.created_at < other.created_at
            || (self.created_at == other.created_at && self.id < other.id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimelineWindowRequest {
    #[serde(default = "default_timeline_window_limit")]
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<TimelineWindowCursor>,
}

impl Default for TimelineWindowRequest {
    fn default() -> Self {
        Self {
            limit: DEFAULT_TIMELINE_WINDOW_LIMIT,
            cursor: None,
        }
    }
}

impl TimelineWindowRequest {
    #[must_use]
    pub fn newest(limit: usize) -> Self {
        Self {
            limit,
            cursor: None,
        }
    }

    #[must_use]
    pub(crate) fn bounded_limit(&self) -> usize {
        if self.limit == 0 {
            DEFAULT_TIMELINE_WINDOW_LIMIT
        } else {
            self.limit.min(MAX_TIMELINE_WINDOW_LIMIT)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimelineWindowPage {
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<TimelineWindowCursor>,
    pub has_more: bool,
    pub total_blocks: usize,
}

const fn default_timeline_window_limit() -> usize {
    DEFAULT_TIMELINE_WINDOW_LIMIT
}

pub(crate) fn sorted_blocks(
    blocks: Vec<TimelineBlock>,
    cards: &BoundedMessageMap<String, TimelineEventCard>,
) -> Vec<TimelineBlock> {
    let mut keyed = blocks
        .into_iter()
        .map(|block| {
            let cursor = block_window_cursor(&block, cards);
            (cursor, block)
        })
        .collect::<Vec<_>>();
    keyed.sort_by(|(left, _), (right, _)| newest_first(left, right));
    keyed.into_iter().map(|(_, block)| block).collect()
}

pub(crate) fn page_start_after_cursor(
    blocks: &[TimelineBlock],
    cards: &BoundedMessageMap<String, TimelineEventCard>,
    cursor: &TimelineWindowCursor,
) -> usize {
    blocks
        .iter()
        .position(|block| block_window_cursor(block, cards).is_older_than(cursor))
        .unwrap_or(blocks.len())
}

pub(crate) fn cards_for_blocks(
    blocks: &[TimelineBlock],
    cards: &BoundedMessageMap<String, TimelineEventCard>,
) -> Vec<TimelineEventCard> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for id in blocks.iter().flat_map(block_event_ids) {
        append_card_and_event_refs(&id, cards, &mut seen, &mut out);
    }
    out
}

fn newest_first(left: &TimelineWindowCursor, right: &TimelineWindowCursor) -> Ordering {
    right
        .created_at
        .cmp(&left.created_at)
        .then_with(|| right.id.cmp(&left.id))
}

fn append_card_and_event_refs(
    id: &str,
    cards: &BoundedMessageMap<String, TimelineEventCard>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<TimelineEventCard>,
) {
    if !seen.insert(id.to_string()) {
        return;
    }
    let Some(card) = cards.get(id).cloned() else {
        return;
    };
    out.push(card.clone());
    for node in &card.content_tree.nodes {
        if let WireNode::EventRef { uri } = node {
            if uri.kind == WireNostrUriKind::Event {
                append_card_and_event_refs(&uri.primary_id, cards, seen, out);
            }
        }
    }
}

pub(crate) fn block_window_cursor(
    block: &TimelineBlock,
    cards: &BoundedMessageMap<String, TimelineEventCard>,
) -> TimelineWindowCursor {
    let event_ids = block_event_ids(block);
    debug_assert!(
        !event_ids.is_empty(),
        "timeline blocks should always carry at least one event id"
    );
    let mut best: Option<TimelineWindowCursor> = None;
    for id in event_ids {
        let cursor = TimelineWindowCursor {
            // The card cache is bounded. If a block outlives its card, keep a
            // stable cursor from the id and push it behind timestamped cards.
            created_at: cards.get(&id).map_or(0, |card| card.created_at),
            id,
        };
        if best
            .as_ref()
            .map_or(true, |existing| cursor.is_newer_than(existing))
        {
            best = Some(cursor);
        }
    }
    best.unwrap_or(TimelineWindowCursor {
        created_at: 0,
        id: String::new(),
    })
}

fn block_event_ids(block: &TimelineBlock) -> Vec<String> {
    match block {
        TimelineBlock::Standalone(id) => vec![id.clone()],
        TimelineBlock::Module { events, .. } => events.clone(),
    }
}
