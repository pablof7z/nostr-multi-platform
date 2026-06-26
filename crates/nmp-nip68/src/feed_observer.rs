use std::sync::Arc;

use nmp_core::substrate::{KernelEvent, SuppressionLookup};
use nmp_core::ObservedProjectionSink;
use nmp_feed::{EventLookup, FlatFeedItem};

use super::{PictureFeed, PictureFeedEntry};
use crate::KIND_PICTURE_EVENT;

/// Observer adapter that applies deletes and caller-owned suppression.
pub struct PictureFeedObserver {
    feed: Arc<PictureFeed>,
    event_lookup: EventLookup,
    suppression: Arc<dyn SuppressionLookup>,
}

#[must_use]
pub fn picture_feed_observer(
    feed: Arc<PictureFeed>,
    event_lookup: EventLookup,
    suppression: Arc<dyn SuppressionLookup>,
) -> Arc<PictureFeedObserver> {
    Arc::new(PictureFeedObserver {
        feed,
        event_lookup,
        suppression,
    })
}

impl ObservedProjectionSink for PictureFeedObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if let Some(record) = nmp_nip18::DeleteRecord::try_from_kernel_event(event) {
            // kind:20 picture events are not addressable, so only the `e`-tag
            // (event-id) targets resolve to a picture row; an `a`-tag target
            // names a coordinate that has no picture row and is a no-op. The
            // delete only removes a source the same author published — at the
            // SOURCE level so removing a deleted repost does not leave (or
            // reanimate) the row from another contribution.
            for target_id in &record.event_targets {
                self.feed.remove_sources_if(|item| {
                    e_tag_delete_matches(item, target_id, &record.author)
                });
            }
            return;
        }

        if event.kind == KIND_PICTURE_EVENT {
            if self.suppression.is_suppressed_author(&event.author)
                || self.suppression.is_suppressed_event(&event.id)
            {
                self.feed.remove_item(&event.id);
                return;
            }
            self.feed.on_kernel_event(event);
            return;
        }

        if event.kind == nmp_nip18::KIND_GENERIC_REPOST {
            if self.remove_if_suppressed_repost_target(event) {
                return;
            }
            if self.suppression.is_suppressed_author(&event.author)
                || self.suppression.is_suppressed_event(&event.id)
            {
                if let Some(target_id) = repost_target_id(event) {
                    self.feed.remove_source(&target_id, &event.id);
                }
                return;
            }
            self.feed.on_kernel_event(event);
        }
    }
}

impl PictureFeedObserver {
    fn remove_if_suppressed_repost_target(&self, event: &KernelEvent) -> bool {
        let Some(record) = nmp_nip18::try_from_kernel_event(event) else {
            return false;
        };
        let Some(target_id) = record.target_event_id else {
            return false;
        };
        if self.suppression.is_suppressed_event(&target_id) {
            self.feed.remove_item(&target_id);
            return true;
        }
        if record
            .embedded_event
            .as_ref()
            .is_some_and(|target| self.suppression.is_suppressed_author(&target.author))
        {
            self.feed.remove_item(&target_id);
            return true;
        }
        if let Some(target) = (self.event_lookup)(&target_id) {
            if self.suppression.is_suppressed_author(&target.author)
                || self.suppression.is_suppressed_event(&target.id)
            {
                self.feed.remove_item(&target_id);
                return true;
            }
        }
        false
    }
}

fn repost_target_id(event: &KernelEvent) -> Option<String> {
    nmp_nip18::try_from_kernel_event(event)?.target_event_id
}

/// Whether an `e`-tag kind:5 delete by `author` targeting `event_id` removes
/// this source contribution (NIP-09 — only the delete author's own events).
///
/// Matches either:
/// * the source event itself (`source_id == event_id`) owned by `author` — the
///   picture event, or a repost wrapper authored by the deleter; or
/// * the **picture event** the source renders (`record.event_id == event_id`)
///   owned by `author` — so a retracted picture is dropped even when it is
///   surfaced through another account's repost (the wrapper id differs).
fn e_tag_delete_matches(
    item: &FlatFeedItem<PictureFeedEntry>,
    event_id: &str,
    author: &str,
) -> bool {
    let source_owned_by_author = match item.card.reposted_by.as_ref() {
        Some(repost) => repost.author_pubkey == author,
        None => item
            .card
            .record
            .as_ref()
            .is_some_and(|record| record.author == author),
    };
    if item.source_id == event_id && source_owned_by_author {
        return true;
    }
    item.card
        .record
        .as_ref()
        .is_some_and(|record| record.event_id == event_id && record.author == author)
}
