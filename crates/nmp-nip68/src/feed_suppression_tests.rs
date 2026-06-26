use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, SuppressionLookup};
use nmp_core::ObservedProjectionSink;
use nmp_feed::FeedRequest;

use super::*;

fn event(id: &str, author: &str, kind: u32, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at,
        tags: vec![vec![
            "imeta".to_string(),
            "url https://cdn.example/a.jpg".to_string(),
            "m image/jpeg".to_string(),
        ]],
        content: "caption".to_string(),
        relay_provenance: vec!["wss://relay.example".to_string()],
    }
}

fn repost(id: &str, author: &str, target: &KernelEvent, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: nmp_nip18::KIND_GENERIC_REPOST,
        created_at,
        tags: vec![vec!["e".to_string(), target.id.clone()]],
        content: serde_json::json!({
            "id": target.id,
            "pubkey": target.author,
            "kind": target.kind,
            "created_at": target.created_at,
            "tags": target.tags,
            "content": target.content,
        })
        .to_string(),
        relay_provenance: vec!["wss://relay.example".to_string()],
    }
}

fn tag_only_picture_repost(
    id: &str,
    author: &str,
    target_id: &str,
    created_at: u64,
) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: nmp_nip18::KIND_GENERIC_REPOST,
        created_at,
        tags: vec![
            vec!["e".to_string(), target_id.to_string()],
            vec!["k".to_string(), KIND_PICTURE_EVENT.to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[derive(Default)]
struct TestSuppression {
    authors: Mutex<HashSet<String>>,
    events: Mutex<HashSet<String>>,
}

impl TestSuppression {
    fn suppress_author(&self, author: &str) {
        self.authors.lock().unwrap().insert(author.to_string());
    }

    fn suppress_event(&self, event_id: &str) {
        self.events.lock().unwrap().insert(event_id.to_string());
    }
}

impl SuppressionLookup for TestSuppression {
    fn is_suppressed_author(&self, author_pubkey: &str) -> bool {
        self.authors
            .lock()
            .map(|authors| authors.contains(author_pubkey))
            .unwrap_or(false)
    }

    fn is_suppressed_event(&self, event_id: &str) -> bool {
        self.events
            .lock()
            .map(|events| events.contains(event_id))
            .unwrap_or(false)
    }
}

#[test]
fn muted_reposter_removes_only_that_source_not_visible_target() {
    let feed = PictureFeed::new(picture_feed_predicate(Arc::new(|_| true)));
    let suppression = Arc::new(TestSuppression::default());
    let observer = picture_feed_observer(feed.clone(), Arc::new(|_| None), suppression.clone());

    observer.on_kernel_event(&event("target", "bob", KIND_PICTURE_EVENT, 20));
    suppression.suppress_author("carol");
    observer.on_kernel_event(&repost(
        "wrapper",
        "carol",
        &event("target", "bob", KIND_PICTURE_EVENT, 20),
        40,
    ));

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1);
    assert_eq!(snapshot.cards[0].card.id, "target");
    assert!(snapshot.cards[0].card.reposted_by.is_none());
}

#[test]
fn muted_embedded_target_author_blocks_repost_wrapper() {
    let feed = PictureFeed::new(picture_feed_predicate(Arc::new(|author| author == "carol")));
    let suppression = Arc::new(TestSuppression::default());
    suppression.suppress_author("bob");
    let observer = picture_feed_observer(feed.clone(), Arc::new(|_| None), suppression);

    observer.on_kernel_event(&repost(
        "wrapper",
        "carol",
        &event("target", "bob", KIND_PICTURE_EVENT, 20),
        40,
    ));

    assert!(feed.is_empty());
}

#[test]
fn muted_cached_target_author_blocks_tag_only_repost() {
    let target = event("target", "bob", KIND_PICTURE_EVENT, 20);
    let feed = PictureFeed::new(picture_feed_predicate(Arc::new(|author| author == "carol")));
    let suppression = Arc::new(TestSuppression::default());
    suppression.suppress_author("bob");
    let observer = picture_feed_observer(
        feed.clone(),
        Arc::new(move |id| {
            if id == "target" {
                Some(target.clone())
            } else {
                None
            }
        }),
        suppression,
    );

    observer.on_kernel_event(&tag_only_picture_repost("wrapper", "carol", "target", 40));

    assert!(feed.is_empty());
}

#[test]
fn suppressing_repost_event_reveals_remaining_target_source() {
    let feed = PictureFeed::new(picture_feed_predicate(Arc::new(|_| true)));
    let suppression = Arc::new(TestSuppression::default());
    let observer = picture_feed_observer(feed.clone(), Arc::new(|_| None), suppression.clone());
    observer.on_kernel_event(&event("target", "bob", KIND_PICTURE_EVENT, 20));
    observer.on_kernel_event(&repost(
        "wrapper",
        "carol",
        &event("target", "bob", KIND_PICTURE_EVENT, 20),
        40,
    ));
    assert_eq!(
        feed.snapshot(&FeedRequest::default()).cards[0]
            .card
            .reposted_by
            .as_ref()
            .unwrap()
            .author_pubkey,
        "carol"
    );

    suppression.suppress_event("wrapper");
    observer.on_kernel_event(&repost(
        "wrapper",
        "carol",
        &event("target", "bob", KIND_PICTURE_EVENT, 20),
        40,
    ));

    let card = &feed.snapshot(&FeedRequest::default()).cards[0].card;
    assert_eq!(card.id, "target");
    assert!(card.reposted_by.is_none());
}
