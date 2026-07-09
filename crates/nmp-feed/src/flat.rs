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
    FeedController, FeedCursor, FeedInterestShape, FeedPage, FeedRequest, FeedWindowPolicy,
    RootCard, RootFeedSnapshot,
};

mod removal;

/// Admission predicate: `true` when an event belongs in this feed.
pub type FlatFeedPredicate = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

/// Converts an admitted event into zero, one, or many canonical feed items.
///
/// Arity is `Vec`, not `Option`, so one source event can fan out into several
/// rows (e.g. a curated-list event surfacing many member rows) as well as the
/// ordinary one-row and zero-row (filtered) cases. The builder MUST be a pure
/// function of the delivered event — no store peek, no ambient state — so the
/// ROWS it returns stay deterministic and replay-order-independent.
///
/// The one narrow, explicitly-permitted exception: a builder MAY perform
/// monotonic, presence-only demand REGISTRATION as a side effect (composite's
/// `crate::LaneMappingRegistry`-driven builders register a `Delivered` ref's
/// target with `DeliveredRefDemand` this way). That side channel never feeds
/// back into the RETURNED rows for the CURRENT call — it only widens which
/// FUTURE events the session's admission/acquisition surface accepts — so it
/// cannot make the builder's output for a given event depend on call order or
/// prior state; only a later, independent call (on the demanded target's own
/// delivery) can be affected, and that call's output is itself a pure
/// function of THAT event once the demand exists. No store peek, no read of
/// mutable ambient state feeds a row's fields.
pub type FlatFeedItemBuilder<C> = Arc<dyn Fn(&KernelEvent) -> Vec<FlatFeedItem<C>> + Send + Sync>;

/// Merge policy when two source events surface the same canonical item id.
pub type FlatFeedMerge<C> =
    Arc<dyn Fn(Option<&FlatFeedItem<C>>, FlatFeedItem<C>) -> FlatFeedItem<C> + Send + Sync>;

/// Fired with a [`FlatFeedItem::source_id`] the instant that source's
/// contribution is dropped from a `FlatFeed` — via `remove_source`/
/// `remove_sources_if` (one source among possibly several sharing a row), or
/// because the whole row containing it was removed outright (`remove_item`/
/// `remove_item_if`, which fires this once per source the removed row held).
///
/// Source granularity, not row granularity, is deliberate: a composite row's
/// canonical id can be the SAME as one of its contributing sources' own
/// declared target (e.g. a comment row-merges onto the article it points at,
/// #3082's `ByTargetCreatedAt` policy), so "the row" is not a stable proxy for
/// "the one declaring event" — the source id is. Composite sessions with
/// `Delivered`-ref demand (#3087) register a hook here so a demand keyed by
/// the declaring event's own id retracts in lockstep with that event's source
/// contribution being removed, instead of growing monotonically for the life
/// of the session. Optional — most feeds have no removal lifecycle observer.
/// Generic and D0-clean — it names no protocol concept, only a source id
/// string.
pub type SourceRemovedHook = Arc<dyn Fn(&str) + Send + Sync>;

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
    window_policy: FeedWindowPolicy,
    visible_limit: AtomicUsize,
    source_removed: Option<SourceRemovedHook>,
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

    /// Construct a flat feed with a covered pull interest and explicit window
    /// policy.
    #[must_use]
    pub fn with_interest_and_window_policy(
        predicate: FlatFeedPredicate,
        item_builder: FlatFeedItemBuilder<C>,
        interest: Option<InterestShape>,
        window_policy: FeedWindowPolicy,
    ) -> Arc<Self> {
        Self::with_merge_and_window_policy(
            predicate,
            item_builder,
            interest,
            default_merge(),
            window_policy,
        )
    }

    /// Construct a flat feed with explicit same-identity merge semantics.
    #[must_use]
    pub fn with_merge(
        predicate: FlatFeedPredicate,
        item_builder: FlatFeedItemBuilder<C>,
        interest: Option<InterestShape>,
        merge: FlatFeedMerge<C>,
    ) -> Arc<Self> {
        Self::with_merge_and_window_policy(
            predicate,
            item_builder,
            interest,
            merge,
            FeedWindowPolicy::default(),
        )
    }

    /// Construct a flat feed with explicit same-identity merge semantics and
    /// app-declared window policy.
    #[must_use]
    pub fn with_merge_and_window_policy(
        predicate: FlatFeedPredicate,
        item_builder: FlatFeedItemBuilder<C>,
        interest: Option<InterestShape>,
        merge: FlatFeedMerge<C>,
        window_policy: FeedWindowPolicy,
    ) -> Arc<Self> {
        Self::with_merge_window_policy_and_source_removed_hook(
            predicate,
            item_builder,
            interest,
            merge,
            window_policy,
            None,
        )
    }

    /// Construct a flat feed with explicit same-identity merge semantics,
    /// app-declared window policy, and an optional [`SourceRemovedHook`] fired
    /// once per source contribution dropped (#3087 — the composite-lane
    /// compiler's `DeliveredRefDemand` retraction wiring is the one caller
    /// that needs this; every other constructor defaults to no hook).
    #[must_use]
    pub fn with_merge_window_policy_and_source_removed_hook(
        predicate: FlatFeedPredicate,
        item_builder: FlatFeedItemBuilder<C>,
        interest: Option<InterestShape>,
        merge: FlatFeedMerge<C>,
        window_policy: FeedWindowPolicy,
        source_removed: Option<SourceRemovedHook>,
    ) -> Arc<Self> {
        let initial_limit = window_policy.initial_visible_limit();
        Arc::new(Self {
            predicate,
            item_builder,
            merge,
            interest,
            state: Mutex::new(FlatFeedState::default()),
            window_policy,
            visible_limit: AtomicUsize::new(initial_limit),
            source_removed,
        })
    }

    fn ingest(&self, event: &KernelEvent) {
        if !(self.predicate)(event) {
            return;
        }
        let incoming_items = (self.item_builder)(event);
        if incoming_items.is_empty() {
            return;
        }
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        for incoming in incoming_items {
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
    /// `(sort_created_at, id)`, windowed to the request limit.
    #[must_use]
    pub fn snapshot(&self, request: &FeedRequest) -> RootFeedSnapshot<C> {
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
        self.window_policy
            .next_visible_limit(current, total)
            .is_some_and(|new_limit| {
                self.visible_limit.store(new_limit, Ordering::Relaxed);
                true
            })
    }

    // `remove_item`/`remove_item_if`/`remove_source`/`remove_sources_if` and
    // their shared `notify_source_removed` helper live in `flat/removal.rs`
    // (split out to stay under the file-size gate; a child module of `flat`
    // reaches these same private fields).

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
        let current = self.visible_limit.load(Ordering::Relaxed);
        self.visible_limit.store(
            self.window_policy.reset_visible_limit(current),
            Ordering::Relaxed,
        );
        had_rows
    }

    /// Snapshot using the current render viewport.
    #[must_use]
    pub fn snapshot_current_window(&self) -> RootFeedSnapshot<C> {
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
