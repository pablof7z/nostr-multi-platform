//! Delivery-time suppression pass for the flat/composite feed engine (#3117).
//!
//! Re-drives mute/delete suppression from events the engine has ALREADY been
//! delivered — never a by-id store peek (the cache-luck bug fixed by #3083's
//! predecessor cleanup; do not reintroduce it here). Mirrors the LIVE
//! `nmp_nip68::PictureFeedObserver` precedent, minus its by-id `EventLookup`
//! repost-target fallback (flagged by #3083 as an anti-pattern, tracked
//! separately — not reproduced here).
//!
//! Wired at BOTH ingest choke points `flat_session.rs` owns: the LIVE
//! `ResyncingObserver::on_kernel_event` sink and the PULL/backfill `apply`
//! closure — so a muted author's post (or a delete) is suppressed regardless
//! of which path delivered it.

use nmp_core::substrate::{KernelEvent, SuppressionLookup};
use nmp_core::ObservedProjectionSink;
use nmp_feed::{FeedRow, FlatFeed};

/// Ingest one delivered event into `feed`, applying suppression first.
///
/// * A kind:5 delete decodes via [`nmp_nip09::DeleteRecord`] (the event's own
///   tags only, zero store lookup) and removes each `e`-tag target's SOURCE
///   contribution — author-gated per NIP-09, matched only against rows the
///   feed already holds ([`FeedRow::author_pubkey`]). This is pure reactive
///   removal: a delete arriving before its target is a harmless no-op (same
///   accepted limit as `PictureFeedObserver`).
/// * Any other event: a suppressed author or event id is dropped from
///   admission AND retro-removed by id — covering a row admitted before the
///   mute existed. Anything else admits normally.
///
/// `a`-tag (addressable) delete targets are NOT handled here — tracked as a
/// follow-up in #3117 (no flat-feed lane on this path currently keys
/// `canonical_row_id` by coordinate; a mapping that does would need a
/// `remove_item_if`-style, `created_at`-gated predicate per NIP-09 §3).
pub(super) fn ingest_with_suppression(
    feed: &FlatFeed<FeedRow>,
    suppression: &dyn SuppressionLookup,
    event: &KernelEvent,
) {
    if let Some(record) = nmp_nip09::DeleteRecord::try_from_kernel_event(event) {
        for target_id in &record.event_targets {
            feed.remove_sources_if(|item| {
                item.source_id == *target_id && item.card.author_pubkey == record.author
            });
        }
        return;
    }

    if suppression.is_suppressed_author(&event.author) || suppression.is_suppressed_event(&event.id)
    {
        feed.remove_item(&event.id);
        return;
    }

    feed.on_kernel_event(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_feed::FlatFeedItem;
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    #[derive(Default)]
    struct TestSuppression {
        authors: RwLock<HashSet<String>>,
        events: RwLock<HashSet<String>>,
    }

    impl TestSuppression {
        fn suppress_author(&self, pubkey: &str) {
            self.authors.write().unwrap().insert(pubkey.to_string());
        }
    }

    impl SuppressionLookup for TestSuppression {
        fn is_suppressed_author(&self, author_pubkey: &str) -> bool {
            self.authors.read().unwrap().contains(author_pubkey)
        }
        fn is_suppressed_event(&self, event_id: &str) -> bool {
            self.events.read().unwrap().contains(event_id)
        }
    }

    fn note(id: &str, author: &str) -> KernelEvent {
        KernelEvent {
            id: id.to_string(),
            author: author.to_string(),
            kind: 1,
            created_at: 1,
            tags: Vec::new(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    fn delete(author: &str, e_targets: &[&str]) -> KernelEvent {
        KernelEvent {
            id: "delete-event".to_string(),
            author: author.to_string(),
            kind: nmp_nip09::KIND_DELETION,
            created_at: 2,
            tags: e_targets
                .iter()
                .map(|id| vec!["e".to_string(), id.to_string()])
                .collect(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    fn new_feed() -> Arc<FlatFeed<FeedRow>> {
        FlatFeed::new(
            Arc::new(|_: &KernelEvent| true),
            Arc::new(|event: &KernelEvent| {
                vec![FlatFeedItem {
                    id: event.id.clone(),
                    source_id: event.id.clone(),
                    sort_created_at: event.created_at,
                    card: FeedRow::from_event(event),
                }]
            }),
        )
    }

    #[test]
    fn suppressed_author_new_post_is_not_admitted() {
        let feed = new_feed();
        let suppression = TestSuppression::default();
        suppression.suppress_author("muted");
        ingest_with_suppression(&feed, &suppression, &note("ev1", "muted"));
        assert_eq!(feed.len(), 0);
    }

    #[test]
    fn post_admitted_before_mute_is_removed_on_redelivery() {
        let feed = new_feed();
        let suppression = Arc::new(TestSuppression::default());
        ingest_with_suppression(&feed, suppression.as_ref(), &note("ev1", "later-muted"));
        assert_eq!(feed.len(), 1);
        suppression.suppress_author("later-muted");
        ingest_with_suppression(&feed, suppression.as_ref(), &note("ev1", "later-muted"));
        assert_eq!(feed.len(), 0);
    }

    #[test]
    fn delete_removes_matching_author_row_only() {
        let feed = new_feed();
        let suppression = TestSuppression::default();
        ingest_with_suppression(&feed, &suppression, &note("target", "alice"));
        assert_eq!(feed.len(), 1);
        // Foreign-author delete for the same id is a no-op (NIP-09 author gate).
        ingest_with_suppression(&feed, &suppression, &delete("mallory", &["target"]));
        assert_eq!(feed.len(), 1);
        // Same-author delete removes it.
        ingest_with_suppression(&feed, &suppression, &delete("alice", &["target"]));
        assert_eq!(feed.len(), 0);
    }

    #[test]
    fn delete_before_target_is_harmless_noop() {
        let feed = new_feed();
        let suppression = TestSuppression::default();
        ingest_with_suppression(
            &feed,
            &suppression,
            &delete("alice", &["not-yet-delivered"]),
        );
        assert_eq!(feed.len(), 0);
        ingest_with_suppression(&feed, &suppression, &note("not-yet-delivered", "alice"));
        // The delete already passed; a later-arriving target survives (same
        // accepted limit as `PictureFeedObserver`, see module doc).
        assert_eq!(feed.len(), 1);
    }
}
