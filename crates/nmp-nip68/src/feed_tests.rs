use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
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

#[test]
fn primary_kind20_acquires_kind16_not_kind6() {
    assert_eq!(
        picture_acquisition_kinds(),
        [KIND_PICTURE_EVENT, nmp_nip18::KIND_GENERIC_REPOST]
            .into_iter()
            .collect()
    );
}

#[test]
fn admits_kind20_and_kind16_from_source_perspective() {
    let feed = PictureFeed::new(picture_feed_predicate(Arc::new(|author| {
        author == "follow"
    })));
    let target = event("target", "outside", KIND_PICTURE_EVENT, 10);
    feed.on_kernel_event(&target);
    assert!(feed.is_empty(), "target author is outside the perspective");

    let wrapper = repost("wrapper", "follow", &target, 30);
    feed.on_kernel_event(&wrapper);
    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1);
    assert_eq!(snapshot.cards[0].card.id, "target");
    assert_eq!(
        snapshot.cards[0]
            .card
            .reposted_by
            .as_ref()
            .unwrap()
            .author_pubkey,
        "follow"
    );
    assert_eq!(
        snapshot.cards[0].card.record.as_ref().unwrap().author,
        "outside"
    );
}

#[test]
fn later_repost_bumps_sort_without_changing_target_timestamp() {
    let feed = PictureFeed::new(picture_feed_predicate(Arc::new(|_| true)));
    let older = event("older", "alice", KIND_PICTURE_EVENT, 10);
    let target = event("target", "bob", KIND_PICTURE_EVENT, 20);
    feed.on_kernel_event(&target);
    feed.on_kernel_event(&older);
    feed.on_kernel_event(&repost("wrapper", "carol", &target, 40));

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards[0].card.id, "target");
    assert_eq!(
        snapshot.cards[0].card.record.as_ref().unwrap().created_at,
        20
    );
    assert_eq!(
        snapshot.cards[0]
            .card
            .reposted_by
            .as_ref()
            .unwrap()
            .repost_created_at,
        40
    );
    assert_eq!(snapshot.cards[1].card.id, "older");
}

#[test]
fn tag_only_repost_creates_placeholder_then_target_hydrates_it() {
    let feed = PictureFeed::new(picture_feed_predicate(Arc::new(|_| true)));
    let wrapper = KernelEvent {
        ..tag_only_picture_repost("wrapper", "carol", "target", 40)
    };
    feed.on_kernel_event(&wrapper);
    assert!(feed.snapshot(&FeedRequest::default()).cards[0]
        .card
        .record
        .is_none());

    feed.on_kernel_event(&event("target", "bob", KIND_PICTURE_EVENT, 20));
    let card = &feed.snapshot(&FeedRequest::default()).cards[0].card;
    assert_eq!(card.id, "target");
    assert_eq!(card.record.as_ref().unwrap().author, "bob");
    assert_eq!(card.reposted_by.as_ref().unwrap().author_pubkey, "carol");
}

#[test]
fn delete_removes_only_author_owned_picture_rows() {
    let feed = PictureFeed::new(picture_feed_predicate(Arc::new(|_| true)));
    let observer = picture_feed_observer(
        feed.clone(),
        Arc::new(|_| None),
        nmp_core::substrate::empty_suppression_lookup(),
    );
    feed.on_kernel_event(&event("target", "bob", KIND_PICTURE_EVENT, 20));

    observer.on_kernel_event(&KernelEvent {
        id: "delete".to_string(),
        author: "mallory".to_string(),
        kind: 5,
        created_at: 30,
        tags: vec![vec!["e".to_string(), "target".to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    });
    assert_eq!(feed.len(), 1);

    observer.on_kernel_event(&KernelEvent {
        id: "delete2".to_string(),
        author: "bob".to_string(),
        kind: 5,
        created_at: 31,
        tags: vec![vec!["e".to_string(), "target".to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    });
    assert!(feed.is_empty());
}

#[test]
fn tag_only_generic_repost_without_picture_kind_claim_is_ignored() {
    let feed = PictureFeed::new(picture_feed_predicate(Arc::new(|_| true)));
    feed.on_kernel_event(&KernelEvent {
        id: "wrapper".to_string(),
        author: "carol".to_string(),
        kind: nmp_nip18::KIND_GENERIC_REPOST,
        created_at: 40,
        tags: vec![vec!["e".to_string(), "longform".to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    });

    assert!(
        feed.is_empty(),
        "kind:16 is shared by all non-kind-1 reposts; picture feeds need a kind:20 claim"
    );
}

#[test]
fn tag_only_repost_hydrates_from_local_event_lookup() {
    let target = event("target", "bob", KIND_PICTURE_EVENT, 20);
    let target_for_lookup = target.clone();
    let feed = PictureFeed::with_event_lookup(
        picture_feed_predicate(Arc::new(|author| author == "carol")),
        Arc::new(move |id| {
            if id == "target" {
                Some(target_for_lookup.clone())
            } else {
                None
            }
        }),
        None,
    );

    feed.on_kernel_event(&tag_only_picture_repost("wrapper", "carol", "target", 40));

    let card = &feed.snapshot(&FeedRequest::default()).cards[0].card;
    assert_eq!(card.id, "target");
    assert_eq!(card.record.as_ref().unwrap().author, "bob");
    assert_eq!(card.reposted_by.as_ref().unwrap().author_pubkey, "carol");
}
