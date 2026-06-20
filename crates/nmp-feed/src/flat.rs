//! Generic predicate-gated flat feed mechanics.
//!
//! This is the substrate-level machine for feeds where every admitted item is a
//! top-level row. Protocol/app crates supply admission, canonical item identity,
//! card construction, and merge semantics. The feed owns only bounded storage,
//! viewport growth, observer ingestion, and pull-controller compatibility.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::planner::InterestShape;
use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use serde::Serialize;

use crate::{
    FeedController, FeedCursor, FeedInterestShape, FeedPage, FeedRequest, RootCard,
    RootFeedSnapshot, DEFAULT_FEED_WINDOW_LIMIT, MAX_FEED_WINDOW_LIMIT,
};

/// Admission predicate: `true` when an event belongs in this feed.
pub type FlatFeedPredicate = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

/// Converts an admitted event into a canonical feed item.
pub type FlatFeedItemBuilder<C> =
    Arc<dyn Fn(&KernelEvent) -> Option<FlatFeedItem<C>> + Send + Sync>;

/// Merge policy when two source events surface the same canonical item id.
pub type FlatFeedMerge<C> =
    Arc<dyn Fn(Option<&FlatFeedItem<C>>, FlatFeedItem<C>) -> FlatFeedItem<C> + Send + Sync>;

/// A renderable flat-feed row keyed by canonical item id.
#[derive(Clone, Debug, PartialEq)]
pub struct FlatFeedItem<C> {
    /// Canonical row identity. For repost-aware feeds this is the target event
    /// id, not the wrapper id.
    pub id: String,
    /// Sort timestamp for this row. A later repost can intentionally sort a
    /// target above its own publish time while the card still renders target
    /// metadata.
    pub sort_created_at: u64,
    pub card: C,
}

#[derive(Clone)]
struct FlatRow<C> {
    sort_created_at: u64,
    card: C,
}

struct FlatFeedState<C> {
    rows: BTreeMap<String, FlatRow<C>>,
}

impl<C> Default for FlatFeedState<C> {
    fn default() -> Self {
        Self {
            rows: BTreeMap::new(),
        }
    }
}

/// A bounded, newest-first flat feed over host/protocol-supplied row semantics.
pub struct FlatFeed<C> {
    predicate: FlatFeedPredicate,
    item_builder: FlatFeedItemBuilder<C>,
    merge: FlatFeedMerge<C>,
    interest: Option<InterestShape>,
    state: Mutex<FlatFeedState<C>>,
    visible_limit: AtomicUsize,
}

impl<C> FlatFeed<C>
where
    C: Clone + Send + Serialize + 'static,
{
    /// Construct a push-only flat feed. `load_older` fails closed unless the
    /// host wraps this feed in a [`crate::PullFeedController`].
    #[must_use]
    pub fn new(predicate: FlatFeedPredicate, item_builder: FlatFeedItemBuilder<C>) -> Arc<Self> {
        Self::with_merge(predicate, item_builder, None, default_merge())
    }

    /// Construct a flat feed with a covered pull interest.
    #[must_use]
    pub fn with_interest(
        predicate: FlatFeedPredicate,
        item_builder: FlatFeedItemBuilder<C>,
        interest: Option<InterestShape>,
    ) -> Arc<Self> {
        Self::with_merge(predicate, item_builder, interest, default_merge())
    }

    /// Construct a flat feed with explicit same-identity merge semantics.
    #[must_use]
    pub fn with_merge(
        predicate: FlatFeedPredicate,
        item_builder: FlatFeedItemBuilder<C>,
        interest: Option<InterestShape>,
        merge: FlatFeedMerge<C>,
    ) -> Arc<Self> {
        Arc::new(Self {
            predicate,
            item_builder,
            merge,
            interest,
            state: Mutex::new(FlatFeedState::default()),
            visible_limit: AtomicUsize::new(DEFAULT_FEED_WINDOW_LIMIT),
        })
    }

    fn ingest(&self, event: &KernelEvent) {
        if !(self.predicate)(event) {
            return;
        }
        let Some(incoming) = (self.item_builder)(event) else {
            return;
        };
        if let Ok(mut st) = self.state.lock() {
            let existing = st.rows.get(&incoming.id).map(|row| FlatFeedItem {
                id: incoming.id.clone(),
                sort_created_at: row.sort_created_at,
                card: row.card.clone(),
            });
            let merged = (self.merge)(existing.as_ref(), incoming);
            st.rows.insert(
                merged.id,
                FlatRow {
                    sort_created_at: merged.sort_created_at,
                    card: merged.card,
                },
            );
        }
    }

    /// Build the visible-window snapshot: cards newest-first by
    /// `(sort_created_at, id)`, windowed to the request limit. Attribution is
    /// empty for flat feeds.
    #[must_use]
    pub fn snapshot(&self, request: &FeedRequest) -> RootFeedSnapshot<C, ()> {
        let Ok(st) = self.state.lock() else {
            return RootFeedSnapshot {
                cards: Vec::new(),
                page: None,
                metrics: None,
            };
        };

        let mut ordered: Vec<(u64, &String, &C)> = st
            .rows
            .iter()
            .map(|(id, row)| (row.sort_created_at, id, &row.card))
            .collect();
        ordered.sort_by(|(lt, lid, _), (rt, rid, _)| rt.cmp(lt).then_with(|| rid.cmp(lid)));

        let limit = request.bounded_limit();
        let total = ordered.len();
        let end = limit.min(total);
        let has_more = end < total;
        let next_cursor = if has_more {
            ordered.get(end - 1).map(|(created_at, id, _)| FeedCursor {
                created_at: *created_at,
                id: (*id).clone(),
            })
        } else {
            None
        };

        let cards = ordered[..end]
            .iter()
            .map(|(_, _, card)| RootCard {
                card: (*card).clone(),
                attribution: Vec::new(),
            })
            .collect::<Vec<_>>();

        RootFeedSnapshot {
            cards,
            page: Some(FeedPage {
                limit,
                next_cursor,
                has_more,
                total_blocks: total,
            }),
            metrics: None,
        }
    }

    /// Grow the render viewport by one page over already-ingested rows.
    pub fn grow_visible_window(&self) -> bool {
        let total = self.state.lock().map(|st| st.rows.len()).unwrap_or(0);
        let current = self.visible_limit.load(Ordering::Relaxed);
        if current >= total || current >= MAX_FEED_WINDOW_LIMIT {
            return false;
        }
        let new_limit = (current + DEFAULT_FEED_WINDOW_LIMIT).min(MAX_FEED_WINDOW_LIMIT);
        self.visible_limit.store(new_limit, Ordering::Relaxed);
        true
    }

    /// Snapshot using the current render viewport.
    #[must_use]
    pub fn snapshot_current_window(&self) -> RootFeedSnapshot<C, ()> {
        let limit = self.visible_limit.load(Ordering::Relaxed);
        self.snapshot(&FeedRequest::newest(limit))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.state.lock().map(|st| st.rows.len()).unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<C> KernelEventObserver for FlatFeed<C>
where
    C: Clone + Send + Serialize + 'static,
{
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ingest(event);
    }
}

impl<C> FeedInterestShape for FlatFeed<C>
where
    C: Clone + Send + Serialize + 'static,
{
    fn interest_shape(&self) -> Option<InterestShape> {
        self.interest.clone()
    }
}

impl<C> FeedController for FlatFeed<C>
where
    C: Clone + Send + Serialize + 'static,
{
    fn load_older(&self) -> bool {
        false
    }
}

fn default_merge<C>() -> FlatFeedMerge<C>
where
    C: Clone + Send + 'static,
{
    Arc::new(|existing, incoming| match existing {
        Some(existing) if existing.sort_created_at > incoming.sort_created_at => existing.clone(),
        _ => incoming,
    })
}

#[cfg(test)]
#[path = "flat_tests.rs"]
mod tests;
