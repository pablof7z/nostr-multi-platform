//! `FlatFeed` — a predicate-gated flat note feed (ADR-0042 §5.1).
//!
//! The M2 author/thread read-path replacement. Unlike the OP-centric root-indexed feed
//! ([`crate::OpFeedEngine`] / [`crate::register_op_feed`]), which is a stream of
//! **thread roots only** with a followed author's replies rolled up as
//! *attribution* metadata, a profile screen and a thread screen each render a
//! **flat list** where every matching note is its own top-level row:
//!
//! * **Author feed** — every primary kind:1 event plus derived kind:6 repost
//!   wrapper authored by one pubkey (including that author's replies to other
//!   people), shown as top-level rows. The root-indexed engine structurally
//!   cannot express this (it would hide the replies under other people's roots).
//! * **Thread feed** — the root note plus every admitted kind:1/kind:6 event
//!   that references it via `#e`, each as its own row (`ThreadScreen` does
//!   `ForEach(thread.items)`).
//!
//! Both are the same machine: a flat, newest-first, D5-windowed list of
//! [`NoteFeedItem`]s, gated by an injected admission predicate. The
//! emitted snapshot is the **same** [`RootFeedSnapshot`] wire shape OP-centric
//! feeds emit (`RootCard { card, attribution }`), with `attribution` always
//! empty — so the iOS/Android shells decode it through the same NNFS
//! `OpFeedSnapshot` schema with zero new FlatBuffers schema or codegen. Apps
//! supply their own projection keys and declare primary kinds (`[1]` for Chirp);
//! the NIP-18 adapter derives the
//! compiled acquisition set (`{1,6}`), and this protocol adapter renders those
//! admitted events. `nmp-core` never owns the primary-kind policy.
//!
//! Registration mirrors [`crate::register_op_feed`]: the host registers a
//! `FlatFeed` as both a [`ObservedProjectionSink`] (ingest fan-out) and a
//! [`FeedController`] under its own snapshot key (`nmp.feed.author.<pk>` /
//! `nmp.feed.thread.<id>`).

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::{
    FeedController, FeedInterestShape, FeedRequest, FlatFeed as GenericFlatFeed, FlatFeedItem,
    FlatFeedItemBuilder, FlatFeedMerge, RootCard, RootFeedSnapshot,
};
use nmp_planner::InterestShape;

use crate::{Nip10ReplyAttribution, NoteFeedItem, RepostAttribution};

pub use nmp_feed::FlatFeedPredicate;

/// A flat, predicate-gated note feed. Wire-compatible with OP-centric feeds'
/// [`RootFeedSnapshot`] (empty `attribution`).
pub struct FlatFeed {
    inner: Arc<GenericFlatFeed<NoteFeedItem>>,
}

impl FlatFeed {
    /// Construct a flat feed admitting events for which `predicate` is `true`.
    ///
    /// The feed has **no** pull interest (`interest_shape() == None`), so a host
    /// that registers it without a pull controller gets projection-only
    /// `load_older` — the historical, fail-closed behaviour. Use
    /// [`Self::with_interest`] to make it pull-pageable.
    #[must_use]
    pub fn new(predicate: FlatFeedPredicate) -> Arc<Self> {
        Arc::new(Self {
            inner: GenericFlatFeed::with_merge(
                predicate,
                event_card_builder(),
                None,
                timeline_merge(),
            ),
        })
    }

    /// Construct a flat feed with both its admission predicate and a covered
    /// pull [`InterestShape`] (e.g. [`author_feed_shape`] / [`thread_feed_shape`]).
    ///
    /// The host pairs this with `nmp_feed::PullFeedController` (built over the
    /// app's `feed_pull_fn`) so `load_older` drains older matching notes by
    /// ingest seq; the predicate still gates ingest and display order stays
    /// `(created_at, id)`. Pass `None` to fail closed (equivalent to
    /// [`Self::new`]).
    #[must_use]
    pub fn with_interest(
        predicate: FlatFeedPredicate,
        interest: Option<InterestShape>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: GenericFlatFeed::with_merge(
                predicate,
                event_card_builder(),
                interest,
                timeline_merge(),
            ),
        })
    }

    #[cfg(test)]
    fn ingest(&self, event: &KernelEvent) {
        self.inner.on_kernel_event(event);
    }

    /// Build the visible-window snapshot: cards newest-first by
    /// `(created_at, id)`, windowed to the request limit (D5). `attribution` is
    /// always empty — a flat feed has no per-root attribution rollup.
    #[must_use]
    pub fn snapshot(
        &self,
        request: &FeedRequest,
    ) -> RootFeedSnapshot<NoteFeedItem, Nip10ReplyAttribution> {
        let snap = self.inner.snapshot(request);
        RootFeedSnapshot {
            cards: snap
                .cards
                .into_iter()
                .map(|card| RootCard {
                    card: card.card,
                    attribution: Vec::new(),
                })
                .collect(),
            page: snap.page,
            metrics: snap.metrics,
        }
    }

    /// Build the visible-window snapshot using the feed's current render
    /// viewport limit. This honors any prior [`Self::grow_visible_window`] call
    /// that widened the viewport beyond the first page.
    #[must_use]
    pub fn snapshot_current_window(&self) -> RootFeedSnapshot<NoteFeedItem, Nip10ReplyAttribution> {
        let snap = self.inner.snapshot_current_window();
        RootFeedSnapshot {
            cards: snap
                .cards
                .into_iter()
                .map(|card| RootCard {
                    card: card.card,
                    attribution: Vec::new(),
                })
                .collect(),
            page: snap.page,
            metrics: snap.metrics,
        }
    }

    /// Number of rows currently held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// `true` when no rows are held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Grow the render viewport by one page over already-ingested rows.
    pub fn grow_visible_window(&self) -> bool {
        self.inner.grow_visible_window()
    }

    /// Clear all rows and reset the visible window to the first page.
    pub fn reset_for_perspective_change(&self) -> bool {
        self.inner.reset_for_perspective_change()
    }
}

fn event_card_builder() -> FlatFeedItemBuilder<NoteFeedItem> {
    Arc::new(|event| {
        let card = NoteFeedItem::from_event_for_op_feed(event, None);
        Some(FlatFeedItem {
            id: card.id.clone(),
            source_id: event.id.clone(),
            sort_created_at: card.created_at,
            card,
        })
    })
}

fn timeline_merge() -> FlatFeedMerge<NoteFeedItem> {
    Arc::new(|existing, incoming| match existing {
        Some(existing) if existing.sort_created_at > incoming.sort_created_at => {
            merge_older_target_into_bumped_row(existing, incoming)
        }
        Some(existing) if existing.sort_created_at == incoming.sort_created_at => {
            prefer_hydrated_card(existing, incoming)
        }
        _ => incoming,
    })
}

fn merge_older_target_into_bumped_row(
    existing: &FlatFeedItem<NoteFeedItem>,
    incoming: FlatFeedItem<NoteFeedItem>,
) -> FlatFeedItem<NoteFeedItem> {
    let Some(existing_repost) = existing.card.reposted_by.clone() else {
        return existing.clone();
    };
    if incoming.card.reposted_by.is_some() {
        return existing.clone();
    }
    let mut card = incoming.card;
    card.reposted_by = Some(RepostAttribution {
        author_pubkey: existing_repost.author_pubkey,
        note_created_at: card.created_at,
    });
    card.created_at = existing.sort_created_at;
    FlatFeedItem {
        id: existing.id.clone(),
        source_id: existing.source_id.clone(),
        sort_created_at: existing.sort_created_at,
        card,
    }
}

fn prefer_hydrated_card(
    existing: &FlatFeedItem<NoteFeedItem>,
    incoming: FlatFeedItem<NoteFeedItem>,
) -> FlatFeedItem<NoteFeedItem> {
    if card_is_placeholder(&existing.card) && !card_is_placeholder(&incoming.card) {
        return FlatFeedItem {
            id: incoming.id,
            source_id: existing.source_id.clone(),
            sort_created_at: existing.sort_created_at,
            card: incoming.card,
        };
    }
    incoming
}

fn card_is_placeholder(card: &NoteFeedItem) -> bool {
    card.content.is_empty() && card.reposted_by.is_some()
}

impl ObservedProjectionSink for FlatFeed {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.inner.on_kernel_event(event);
    }
}

impl FeedInterestShape for FlatFeed {
    /// The feed's covered pull interest, or `None` to fail closed (ADR-0058 §8
    /// 6B). The host pairs the feed with a `nmp_feed::PullFeedController`, which
    /// is constructed UNCONDITIONALLY; its `load_older` re-reads this shape on
    /// every call and fails closed (returns `false`, no pull, no broad-scan)
    /// whenever it yields `None`.
    fn interest_shape(&self) -> Option<InterestShape> {
        self.inner.interest_shape()
    }
}

impl FeedController for FlatFeed {
    fn load_older(&self) -> bool {
        self.inner.load_older()
    }
}

/// Build the **author-feed** pull [`InterestShape`]: `{authors:[pk], kinds}` —
/// the covered E1 `AuthorsKind` shape the kernel pull substrate maps to
/// `idx_kind_author_time` (ADR-0045). Pair with [`author_feed_predicate`] and a
/// `nmp_feed::PullFeedController` so `load_older` drains older notes by that
/// author. The `kinds` argument is the compiled acquisition set derived from the
/// app's primary-kind declaration.
#[must_use]
pub fn author_feed_shape(author: String, kinds: Vec<u32>) -> InterestShape {
    InterestShape::timeline_for(
        BTreeSet::from([author]),
        kinds.into_iter().collect::<BTreeSet<u32>>(),
    )
}

/// Build the **thread-feed reply-tail** pull [`InterestShape`]:
/// `{kinds, #e:[root]}` — the covered E2 `Etag` shape. This pages the *replies*
/// that reference `root_id` via `#e`; the root note itself is an event-id-only
/// interest that the pull substrate does not cover, so the screen/component that
/// needs the root owns that separate dependency (ADR-0058 §8 6B). Pair with
/// [`thread_feed_predicate`] (which still admits the root by id) and a
/// `nmp_feed::PullFeedController`.
#[must_use]
pub fn thread_feed_shape(root_id: String, kinds: Vec<u32>) -> InterestShape {
    let mut shape = InterestShape {
        kinds: kinds.into_iter().collect::<BTreeSet<u32>>(),
        ..Default::default()
    };
    shape
        .tags
        .insert("e".to_string(), BTreeSet::from([root_id]));
    shape
}

/// Build an **author-feed** predicate: a compiled acquisition kind set authored
/// by one pubkey. For a primary kind `[1]` feed, the caller passes the adapter-
/// derived `{1,6}` set; the substrate never chooses that policy.
///
/// `kinds` is the compiled acquisition kind set. `author` is the raw hex pubkey.
#[must_use]
pub fn author_feed_predicate(author: String, kinds: Vec<u32>) -> FlatFeedPredicate {
    Arc::new(move |event: &KernelEvent| event.author == author && kinds.contains(&event.kind))
}

/// Build a **thread-feed** predicate: the root note itself plus every event of
/// a host-chosen kind that references the root via a NIP-10 `#e` tag.
///
/// Crucially this admits the root event by id (`event.id == root_id`) — a
/// a compiled acquisition filter with only `#e=[root]` would fetch the
/// *replies* but not the root, and `ThreadScreen` must show the root as a row.
/// The `#e` match is any `e` tag whose value equals `root_id` (NIP-10 root or
/// reply marker).
#[must_use]
pub fn thread_feed_predicate(root_id: String, kinds: Vec<u32>) -> FlatFeedPredicate {
    Arc::new(move |event: &KernelEvent| {
        if event.id == root_id {
            return true;
        }
        if !kinds.contains(&event.kind) {
            return false;
        }
        event
            .tags
            .iter()
            .any(|tag| tag.first().map(String::as_str) == Some("e") && tag.get(1) == Some(&root_id))
    })
}

#[cfg(test)]
#[path = "flat_feed/tests.rs"]
mod tests;
