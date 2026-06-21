use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_feed::FeedRequest;

use super::*;

const AUTHOR_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AUTHOR_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const AUTHOR_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn address(author: &str, d_tag: &str) -> String {
    format!("{KIND_LONG_FORM_ARTICLE}:{author}:{d_tag}")
}

fn article(id: &str, author: &str, d_tag: &str, created_at: u64, topic: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_LONG_FORM_ARTICLE,
        created_at,
        tags: vec![
            vec!["d".to_string(), d_tag.to_string()],
            vec!["title".to_string(), format!("title {d_tag}")],
            vec!["summary".to_string(), format!("summary {d_tag}")],
            vec![
                "image".to_string(),
                format!("https://img.example/{d_tag}.jpg"),
            ],
            vec!["t".to_string(), topic.to_string()],
        ],
        content: format!("body {d_tag}"),
        relay_provenance: vec!["wss://relay.example".to_string()],
    }
}

fn embedded_repost(id: &str, author: &str, target: &KernelEvent, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: nmp_nip18::KIND_GENERIC_REPOST,
        created_at,
        tags: vec![
            vec!["e".to_string(), target.id.clone()],
            vec!["k".to_string(), KIND_LONG_FORM_ARTICLE.to_string()],
        ],
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

fn tag_only_repost(id: &str, author: &str, target_id: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: nmp_nip18::KIND_GENERIC_REPOST,
        created_at,
        tags: vec![
            vec!["e".to_string(), target_id.to_string()],
            vec!["k".to_string(), KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn primary_kind30023_acquires_kind16_not_kind6() {
    assert_eq!(
        longform_acquisition_kinds(),
        [KIND_LONG_FORM_ARTICLE, nmp_nip18::KIND_GENERIC_REPOST]
            .into_iter()
            .collect()
    );
}

#[test]
fn kind16_embedded_article_repost_uses_address_identity_and_wrapper_sort() {
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|author| {
        author == AUTHOR_C
    })));
    let target = article("target", AUTHOR_A, "article-a", 10, "nostr");
    feed.on_kernel_event(&target);
    assert!(
        feed.is_empty(),
        "target author is outside source perspective"
    );

    feed.on_kernel_event(&embedded_repost("wrapper", AUTHOR_C, &target, 40));

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1);
    let row = &snapshot.cards[0].card;
    assert_eq!(row.id, address(AUTHOR_A, "article-a"));
    assert_eq!(row.article.as_ref().unwrap().id, "target");
    assert_eq!(row.article.as_ref().unwrap().created_at, 10);
    assert_eq!(row.reposted_by.as_ref().unwrap().author_pubkey, AUTHOR_C);
    assert_eq!(row.reposted_by.as_ref().unwrap().repost_created_at, 40);
}

#[test]
fn tag_only_kind16_uses_injected_local_lookup_without_claiming_target() {
    let target = article("target", AUTHOR_A, "article-a", 10, "nostr");
    let target_for_lookup = target.clone();
    let feed = LongformFeed::with_event_lookup(
        longform_feed_predicate(Arc::new(|author| author == AUTHOR_C)),
        Arc::new(move |id| {
            if id == "target" {
                Some(target_for_lookup.clone())
            } else {
                None
            }
        }),
    );

    feed.on_kernel_event(&tag_only_repost("wrapper", AUTHOR_C, "target", 40));

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1);
    let row = &snapshot.cards[0].card;
    assert_eq!(row.id, address(AUTHOR_A, "article-a"));
    assert_eq!(row.article.as_ref().unwrap().author_pubkey, AUTHOR_A);
    assert_eq!(row.reposted_by.as_ref().unwrap().repost_event_id, "wrapper");
}

#[test]
fn later_wrapper_bumps_order_without_changing_article_created_at() {
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    let older = article("older", AUTHOR_B, "older", 20, "nostr");
    let target = article("target", AUTHOR_A, "target", 10, "nostr");
    feed.on_kernel_event(&older);
    feed.on_kernel_event(&target);
    feed.on_kernel_event(&embedded_repost("wrapper", AUTHOR_C, &target, 50));

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards[0].card.id, address(AUTHOR_A, "target"));
    assert_eq!(
        snapshot.cards[0].card.article.as_ref().unwrap().created_at,
        10
    );
    assert_eq!(snapshot.cards[1].card.id, address(AUTHOR_B, "older"));
}

#[test]
fn non_30023_kind16_is_ignored() {
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    feed.on_kernel_event(&KernelEvent {
        id: "wrapper".to_string(),
        author: AUTHOR_C.to_string(),
        kind: nmp_nip18::KIND_GENERIC_REPOST,
        created_at: 40,
        tags: vec![
            vec!["e".to_string(), "target".to_string()],
            vec!["k".to_string(), "20".to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    });

    assert!(feed.is_empty());
}

#[test]
fn topic_feed_ignores_unresolved_tag_only_repost() {
    let feed = LongformFeed::for_topic(
        "nostr",
        longform_feed_predicate(Arc::new(|author| author == AUTHOR_C)),
        Arc::new(|_| None),
    );

    feed.on_kernel_event(&tag_only_repost("wrapper", AUTHOR_C, "target", 40));

    assert!(
        feed.is_empty(),
        "topic membership cannot be proven without embedded or local target data"
    );
}

#[test]
fn topic_feed_admits_repost_only_when_target_matches_topic() {
    let feed = LongformFeed::for_topic(
        "nostr",
        longform_feed_predicate(Arc::new(|author| author == AUTHOR_C)),
        Arc::new(|_| None),
    );
    let matching = article("target", AUTHOR_A, "article-a", 10, "nostr");
    let other = article("other", AUTHOR_A, "article-b", 20, "music");

    feed.on_kernel_event(&embedded_repost("wrapper-other", AUTHOR_C, &other, 30));
    feed.on_kernel_event(&embedded_repost("wrapper-match", AUTHOR_C, &matching, 40));

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1);
    assert_eq!(snapshot.cards[0].card.id, address(AUTHOR_A, "article-a"));
}

#[test]
fn unresolved_tag_only_kind16_is_ignored_without_fetching_target() {
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));

    feed.on_kernel_event(&tag_only_repost("wrapper", AUTHOR_C, "target", 40));

    assert!(
        feed.is_empty(),
        "longform address identity cannot be proven from an event-id-only repost"
    );
}

#[test]
fn replaceable_article_repost_dedupes_by_address_and_keeps_freshest_article() {
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    let v1 = article("event-v1", AUTHOR_A, "same-article", 10, "nostr");
    let v2 = article("event-v2", AUTHOR_A, "same-article", 40, "nostr");
    let v3 = article("event-v3", AUTHOR_A, "same-article", 60, "nostr");

    feed.on_kernel_event(&v1);
    feed.on_kernel_event(&embedded_repost("wrapper", AUTHOR_C, &v1, 50));
    feed.on_kernel_event(&v2);

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1);
    let row = &snapshot.cards[0].card;
    assert_eq!(row.id, address(AUTHOR_A, "same-article"));
    assert_eq!(row.article.as_ref().unwrap().id, "event-v2");
    assert_eq!(row.reposted_by.as_ref().unwrap().repost_event_id, "wrapper");

    feed.on_kernel_event(&v3);

    let snapshot = feed.snapshot(&FeedRequest::default());
    let row = &snapshot.cards[0].card;
    assert_eq!(row.article.as_ref().unwrap().id, "event-v3");
    assert!(
        row.reposted_by.is_none(),
        "newer direct article now positions the row"
    );
}
