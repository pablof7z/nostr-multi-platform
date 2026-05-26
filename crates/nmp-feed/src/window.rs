use std::{cmp::Ordering, collections::BTreeSet};

use crate::{
    FeedBlock, FeedCard, FeedCardStore, FeedCursor, FeedPage, FeedRequest, FeedWindowState,
    DEFAULT_FEED_WINDOW_LIMIT, MAX_FEED_WINDOW_LIMIT,
};

impl FeedWindowState {
    pub fn snapshot_blocks<B, C, S>(&self, blocks: &[B], cards: &S) -> (Vec<B>, FeedPage)
    where
        B: FeedBlock,
        C: FeedCard,
        S: FeedCardStore<C>,
    {
        let total_blocks = blocks.len();
        let end = self.current_end(blocks, cards);
        let page_blocks = blocks[..end].to_vec();
        let has_more = end < total_blocks && end < MAX_FEED_WINDOW_LIMIT;
        let next_cursor = if has_more {
            page_blocks.last().map(|block| block_cursor(block, cards))
        } else {
            None
        };
        (
            page_blocks,
            FeedPage {
                limit: end,
                next_cursor,
                has_more,
                total_blocks,
            },
        )
    }

    pub fn load_older<B, C, S>(&mut self, blocks: &[B], cards: &S) -> bool
    where
        B: FeedBlock,
        C: FeedCard,
        S: FeedCardStore<C>,
    {
        let total_blocks = blocks.len();
        let current_end = self.current_end(blocks, cards);
        if current_end >= total_blocks || current_end >= MAX_FEED_WINDOW_LIMIT {
            return false;
        }
        let next_end = current_end
            .saturating_add(DEFAULT_FEED_WINDOW_LIMIT)
            .min(total_blocks)
            .min(MAX_FEED_WINDOW_LIMIT);
        if next_end == current_end {
            return false;
        }
        self.oldest_visible = blocks
            .get(next_end - 1)
            .map(|block| block_cursor(block, cards));
        true
    }

    fn current_end<B, C, S>(&self, blocks: &[B], cards: &S) -> usize
    where
        B: FeedBlock,
        C: FeedCard,
        S: FeedCardStore<C>,
    {
        let total_blocks = blocks.len();
        if total_blocks == 0 {
            return 0;
        }
        let default_end = DEFAULT_FEED_WINDOW_LIMIT
            .min(total_blocks)
            .min(MAX_FEED_WINDOW_LIMIT);
        let cursor_end = self
            .oldest_visible
            .as_ref()
            .map(|cursor| page_start_after_cursor(blocks, cards, cursor))
            .unwrap_or(default_end);
        cursor_end
            .max(default_end)
            .min(total_blocks)
            .min(MAX_FEED_WINDOW_LIMIT)
    }
}

pub fn page_for_request<B, C, S>(
    blocks: &[B],
    cards: &S,
    request: &FeedRequest,
) -> (Vec<B>, FeedPage)
where
    B: FeedBlock,
    C: FeedCard,
    S: FeedCardStore<C>,
{
    let total_blocks = blocks.len();
    let limit = request.bounded_limit();
    let start = request
        .cursor
        .as_ref()
        .map(|cursor| page_start_after_cursor(blocks, cards, cursor))
        .unwrap_or(0);
    let end = start.saturating_add(limit).min(total_blocks);
    let page_blocks = blocks[start..end].to_vec();
    let has_more = end < total_blocks;
    let next_cursor = if has_more {
        page_blocks.last().map(|block| block_cursor(block, cards))
    } else {
        None
    };
    (
        page_blocks,
        FeedPage {
            limit,
            next_cursor,
            has_more,
            total_blocks,
        },
    )
}

pub fn sorted_blocks<B, C, S>(blocks: Vec<B>, cards: &S) -> Vec<B>
where
    B: FeedBlock,
    C: FeedCard,
    S: FeedCardStore<C>,
{
    let mut keyed = blocks
        .into_iter()
        .map(|block| {
            let cursor = block_cursor(&block, cards);
            (cursor, block)
        })
        .collect::<Vec<_>>();
    keyed.sort_by(|(left, _), (right, _)| newest_first(left, right));
    keyed.into_iter().map(|(_, block)| block).collect()
}

pub fn cards_for_blocks<B, C, S>(blocks: &[B], cards: &S) -> Vec<C>
where
    B: FeedBlock,
    C: FeedCard,
    S: FeedCardStore<C>,
{
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for id in blocks.iter().flat_map(FeedBlock::feed_event_ids) {
        append_card_and_event_refs(&id, cards, &mut seen, &mut out);
    }
    out
}

pub fn block_cursor<B, C, S>(block: &B, cards: &S) -> FeedCursor
where
    B: FeedBlock,
    C: FeedCard,
    S: FeedCardStore<C>,
{
    let event_ids = block.feed_event_ids();
    debug_assert!(
        !event_ids.is_empty(),
        "feed blocks should always carry at least one event id"
    );
    let mut best: Option<FeedCursor> = None;
    for id in event_ids {
        let cursor = FeedCursor {
            // Card caches are bounded. If a block outlives its card, keep a
            // stable cursor from the id and push it behind timestamped cards.
            created_at: cards.feed_card(&id).map_or(0, FeedCard::feed_created_at),
            id,
        };
        if best
            .as_ref()
            .map_or(true, |existing| cursor.is_newer_than(existing))
        {
            best = Some(cursor);
        }
    }
    best.unwrap_or(FeedCursor {
        created_at: 0,
        id: String::new(),
    })
}

fn page_start_after_cursor<B, C, S>(blocks: &[B], cards: &S, cursor: &FeedCursor) -> usize
where
    B: FeedBlock,
    C: FeedCard,
    S: FeedCardStore<C>,
{
    blocks
        .iter()
        .position(|block| block_cursor(block, cards).is_older_than(cursor))
        .unwrap_or(blocks.len())
}

fn newest_first(left: &FeedCursor, right: &FeedCursor) -> Ordering {
    right
        .created_at
        .cmp(&left.created_at)
        .then_with(|| right.id.cmp(&left.id))
}

fn append_card_and_event_refs<C, S>(
    id: &str,
    cards: &S,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<C>,
) where
    C: FeedCard,
    S: FeedCardStore<C>,
{
    if !seen.insert(id.to_string()) {
        return;
    }
    let Some(card) = cards.feed_card(id).cloned() else {
        return;
    };
    out.push(card.clone());
    for event_id in card.feed_event_refs() {
        append_card_and_event_refs(&event_id, cards, seen, out);
    }
}
