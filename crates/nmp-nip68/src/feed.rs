//! NIP-68 picture feed adapter over generic `nmp-feed` mechanics.
//!
//! Apps declare primary kind `20`; this adapter admits kind:20 picture events
//! and NIP-18 kind:16 repost wrappers. The app supplies the perspective
//! predicate (follows, WoT, relay-set, search, local model score, etc.) and
//! renders app-specific cards from [`PictureFeedEntry`].

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_feed::{
    EventLookup, FeedController, FeedInterestShape, FeedRequest, FlatFeed as GenericFlatFeed,
    FlatFeedItem, FlatFeedItemBuilder, FlatFeedMerge, RootFeedSnapshot,
};
use nmp_planner::InterestShape;
use serde::{Deserialize, Serialize};

use crate::{try_from_kernel_event, PictureEventRecord, KIND_PICTURE_EVENT};

#[path = "feed_observer.rs"]
mod feed_observer;
pub use feed_observer::{picture_feed_observer, PictureFeedObserver};

/// Admission predicate supplied by the app/protocol composition layer.
pub type PictureFeedPredicate = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureRepostAttribution {
    pub author_pubkey: String,
    pub repost_event_id: String,
    pub repost_created_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureFeedEntry {
    /// Canonical feed row id: the target picture event id, not the repost id.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<PictureEventRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reposted_by: Option<PictureRepostAttribution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relay_provenance: Vec<String>,
}

pub struct PictureFeed {
    inner: Arc<GenericFlatFeed<PictureFeedEntry>>,
}

impl PictureFeed {
    #[must_use]
    pub fn new(predicate: PictureFeedPredicate) -> Arc<Self> {
        Self::with_event_lookup(predicate, Arc::new(|_| None), None)
    }

    #[must_use]
    pub fn with_interest(
        predicate: PictureFeedPredicate,
        interest: Option<InterestShape>,
    ) -> Arc<Self> {
        Self::with_event_lookup(predicate, Arc::new(|_| None), interest)
    }

    #[must_use]
    pub fn with_event_lookup(
        predicate: PictureFeedPredicate,
        event_lookup: EventLookup,
        interest: Option<InterestShape>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: GenericFlatFeed::with_merge(
                predicate,
                picture_item_builder(event_lookup),
                interest,
                picture_merge(),
            ),
        })
    }

    #[must_use]
    pub fn snapshot(&self, request: &FeedRequest) -> RootFeedSnapshot<PictureFeedEntry, ()> {
        self.inner.snapshot(request)
    }

    #[must_use]
    pub fn snapshot_current_window(&self) -> RootFeedSnapshot<PictureFeedEntry, ()> {
        self.inner.snapshot_current_window()
    }

    pub fn grow_visible_window(&self) -> bool {
        self.inner.grow_visible_window()
    }

    pub fn remove_item(&self, id: &str) -> bool {
        self.inner.remove_item(id)
    }

    pub fn remove_item_if(
        &self,
        id: &str,
        predicate: impl FnOnce(&PictureFeedEntry) -> bool,
    ) -> bool {
        self.inner.remove_item_if(id, predicate)
    }

    pub fn remove_source(&self, id: &str, source_id: &str) -> bool {
        self.inner.remove_source(id, source_id)
    }

    pub fn remove_sources_if(
        &self,
        predicate: impl Fn(&FlatFeedItem<PictureFeedEntry>) -> bool,
    ) -> usize {
        self.inner.remove_sources_if(predicate)
    }

    pub fn reset_for_perspective_change(&self) -> bool {
        self.inner.reset_for_perspective_change()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl KernelEventObserver for PictureFeed {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.inner.on_kernel_event(event);
    }
}

impl FeedInterestShape for PictureFeed {
    fn interest_shape(&self) -> Option<InterestShape> {
        self.inner.interest_shape()
    }
}

impl FeedController for PictureFeed {
    fn load_older(&self) -> bool {
        self.inner.load_older()
    }
}

#[must_use]
pub fn picture_acquisition_kinds() -> BTreeSet<u32> {
    nmp_nip18::acquisition_kinds_for_primary([KIND_PICTURE_EVENT])
}

#[must_use]
pub fn picture_feed_shape(authors: BTreeSet<String>) -> InterestShape {
    InterestShape::timeline_for(authors, picture_acquisition_kinds())
}

/// Build a source-event predicate for a picture feed.
///
/// For a primary event the source is the picture author. For a repost wrapper
/// the source is the reposter; the target author is intentionally not used for
/// admission because the repost is what makes the target appear in this
/// perspective.
#[must_use]
pub fn picture_feed_predicate(
    source_allows: Arc<dyn Fn(&str) -> bool + Send + Sync>,
) -> PictureFeedPredicate {
    Arc::new(move |event: &KernelEvent| {
        (event.kind == KIND_PICTURE_EVENT || event.kind == nmp_nip18::KIND_GENERIC_REPOST)
            && source_allows(&event.author)
    })
}

fn picture_item_builder(event_lookup: EventLookup) -> FlatFeedItemBuilder<PictureFeedEntry> {
    Arc::new(move |event| match event.kind {
        KIND_PICTURE_EVENT => picture_item_from_target(event),
        nmp_nip18::KIND_GENERIC_REPOST => picture_item_from_repost(event, &event_lookup),
        _ => None,
    })
}

fn picture_item_from_target(event: &KernelEvent) -> Option<FlatFeedItem<PictureFeedEntry>> {
    let record = try_from_kernel_event(event)?;
    Some(FlatFeedItem {
        id: record.event_id.clone(),
        source_id: event.id.clone(),
        sort_created_at: record.created_at,
        card: PictureFeedEntry {
            id: record.event_id.clone(),
            record: Some(record),
            reposted_by: None,
            relay_provenance: event.relay_provenance.clone(),
        },
    })
}

fn picture_item_from_repost(
    event: &KernelEvent,
    event_lookup: &EventLookup,
) -> Option<FlatFeedItem<PictureFeedEntry>> {
    let record = nmp_nip18::try_from_kernel_event(event)?;
    let target_id = record.target_event_id.clone()?;
    let target = picture_record_from_repost_target(&record, event, event_lookup);
    if target.is_none() && record.target_kind != Some(KIND_PICTURE_EVENT) {
        return None;
    }
    let id = target
        .as_ref()
        .map_or_else(|| target_id.clone(), |target| target.event_id.clone());

    Some(FlatFeedItem {
        id: id.clone(),
        source_id: event.id.clone(),
        sort_created_at: event.created_at,
        card: PictureFeedEntry {
            id,
            record: target,
            reposted_by: Some(PictureRepostAttribution {
                author_pubkey: event.author.clone(),
                repost_event_id: event.id.clone(),
                repost_created_at: event.created_at,
            }),
            relay_provenance: event.relay_provenance.clone(),
        },
    })
}

fn picture_record_from_repost_target(
    record: &nmp_nip18::RepostRecord,
    event: &KernelEvent,
    event_lookup: &EventLookup,
) -> Option<PictureEventRecord> {
    if let Some(embedded) = record.embedded_event.clone() {
        let target_event = KernelEvent {
            id: embedded.id,
            author: embedded.author,
            kind: embedded.kind,
            created_at: embedded.created_at,
            tags: embedded.tags,
            content: embedded.content,
            relay_provenance: event.relay_provenance.clone(),
        };
        return try_from_kernel_event(&target_event);
    }
    record
        .target_event_id
        .as_ref()
        .and_then(|target_id| (event_lookup)(target_id))
        .and_then(|target| try_from_kernel_event(&target))
}

fn picture_merge() -> FlatFeedMerge<PictureFeedEntry> {
    Arc::new(|existing, incoming| match existing {
        Some(existing) => merge_picture_sources(existing, incoming),
        None => incoming,
    })
}

fn merge_picture_sources(
    existing: &FlatFeedItem<PictureFeedEntry>,
    incoming: FlatFeedItem<PictureFeedEntry>,
) -> FlatFeedItem<PictureFeedEntry> {
    let incoming_wins = incoming.sort_created_at > existing.sort_created_at
        || (incoming.sort_created_at == existing.sort_created_at
            && incoming.card.record.is_some()
            && existing.card.record.is_none());
    let (mut best, other) = if incoming_wins {
        (incoming, existing.clone())
    } else {
        (existing.clone(), incoming)
    };

    if best.card.record.is_none() {
        best.card.record = other.card.record.clone();
        if best.card.relay_provenance.is_empty() {
            best.card.relay_provenance = other.card.relay_provenance;
        }
    }
    best
}

#[cfg(test)]
#[path = "feed_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "feed_suppression_tests.rs"]
mod suppression_tests;
