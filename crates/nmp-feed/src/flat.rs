//! Generic predicate-gated flat feed mechanics.
//!
//! This is the substrate-level machine for feeds where every admitted item is a
//! top-level row. Protocol/app crates supply admission, canonical item identity,
//! card construction, and merge semantics. The feed owns only bounded storage,
//! viewport growth, observer ingestion, and pull-controller compatibility.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_planner::InterestShape;
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
    /// Source event identity that contributed this canonical row.
    ///
    /// A target event and one or more repost wrappers can all surface the same
    /// canonical row. Keeping source identity lets protocol adapters remove one
    /// contribution, then let the feed recompute the remaining best row.
    pub source_id: String,
    /// Sort timestamp for this row. A later repost can intentionally sort a
    /// target above its own publish time while the card still renders target
    /// metadata.
    pub sort_created_at: u64,
    pub card: C,
}

#[derive(Clone)]
struct FlatRow<C> {
    sources: BTreeMap<String, FlatFeedItem<C>>,
    best: FlatFeedItem<C>,
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
            let row_id = incoming.id.clone();
            let source_id = incoming.source_id.clone();
            match st.rows.get_mut(&row_id) {
                Some(row) => {
                    row.sources.insert(source_id, incoming);
                    if let Some(best) = merge_sources(&self.merge, &row.sources) {
                        row.best = best;
                    }
                }
                None => {
                    let mut sources = BTreeMap::new();
                    sources.insert(source_id, incoming.clone());
                    st.rows.insert(
                        row_id,
                        FlatRow {
                            sources,
                            best: incoming,
                        },
                    );
                }
            }
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
            .map(|(id, row)| (row.best.sort_created_at, id, &row.best.card))
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

    /// Remove an entire canonical row by id.
    ///
    /// Protocol adapters use this for deletes, mutes, blocks, and other
    /// externally-owned suppression facts that apply to the target event. The
    /// generic feed does not interpret those policies; it only owns row-index
    /// mutation.
    pub fn remove_item(&self, id: &str) -> bool {
        self.state
            .lock()
            .ok()
            .and_then(|mut st| st.rows.remove(id))
            .is_some()
    }

    /// Remove an entire canonical row when the current best card satisfies
    /// `predicate`.
    pub fn remove_item_if(&self, id: &str, predicate: impl FnOnce(&C) -> bool) -> bool {
        let Ok(mut st) = self.state.lock() else {
            return false;
        };
        let should_remove = st.rows.get(id).is_some_and(|row| predicate(&row.best.card));
        if should_remove {
            st.rows.remove(id);
            true
        } else {
            false
        }
    }

    /// Remove one source contribution from a canonical row and recompute the
    /// row from remaining sources.
    pub fn remove_source(&self, id: &str, source_id: &str) -> bool {
        let Ok(mut st) = self.state.lock() else {
            return false;
        };
        let Some(row) = st.rows.get_mut(id) else {
            return false;
        };
        if row.sources.remove(source_id).is_none() {
            return false;
        }
        if let Some(best) = merge_sources(&self.merge, &row.sources) {
            row.best = best;
        } else {
            st.rows.remove(id);
        }
        true
    }

    /// Remove all source contributions matching `predicate`.
    ///
    /// Returns the number of removed sources. Canonical rows with remaining
    /// sources are recomputed; rows with no sources left are removed.
    pub fn remove_sources_if(&self, predicate: impl Fn(&FlatFeedItem<C>) -> bool) -> usize {
        let Ok(mut st) = self.state.lock() else {
            return 0;
        };

        let mut removed = 0usize;
        let mut empty_rows = Vec::new();
        for (id, row) in &mut st.rows {
            let before = row.sources.len();
            row.sources.retain(|_, item| !predicate(item));
            removed += before.saturating_sub(row.sources.len());
            if let Some(best) = merge_sources(&self.merge, &row.sources) {
                row.best = best;
            } else {
                empty_rows.push(id.clone());
            }
        }
        for id in empty_rows {
            st.rows.remove(&id);
        }
        removed
    }

    /// Clear all rows and return the visible window to the first page.
    ///
    /// App/protocol adapters call this when the perspective changes in a way
    /// that invalidates every current admission decision (account switch, follow
    /// set replacement, WoT preset change). The feed does not reinterpret the
    /// predicate over historical rows; it drops the stale window and lets live
    /// ingest/cache-serve refill it under the new perspective.
    pub fn reset_for_perspective_change(&self) -> bool {
        let Ok(mut st) = self.state.lock() else {
            return false;
        };
        let had_rows = !st.rows.is_empty();
        st.rows.clear();
        self.visible_limit
            .store(DEFAULT_FEED_WINDOW_LIMIT, Ordering::Relaxed);
        had_rows
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

impl<C> ObservedProjectionSink for FlatFeed<C>
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
        Some(existing) if existing.sort_created_at >= incoming.sort_created_at => existing.clone(),
        _ => incoming,
    })
}

fn merge_sources<C>(
    merge: &FlatFeedMerge<C>,
    sources: &BTreeMap<String, FlatFeedItem<C>>,
) -> Option<FlatFeedItem<C>>
where
    C: Clone + Send + 'static,
{
    let mut items = sources.values().cloned().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .sort_created_at
            .cmp(&left.sort_created_at)
            .then_with(|| right.source_id.cmp(&left.source_id))
    });
    let mut items = items.into_iter();
    let mut merged = items.next()?;
    for item in items {
        merged = merge(Some(&merged), item);
    }
    Some(merged)
}

#[cfg(test)]
#[path = "flat_tests.rs"]
mod tests;
