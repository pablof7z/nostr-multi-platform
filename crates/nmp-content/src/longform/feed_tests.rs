use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::FeedRequest;

use super::*;

const AUTHOR_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AUTHOR_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const AUTHOR_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn address(author: &str, d_tag: &str) -> String {
    // Use the canonical identity primitive (not a hand-rolled format!) so these
    // tests cannot pass by mirroring a buggy wire string — they exercise the
    // same `AddressCoordinate` the production row id is built from.
    nmp_nip18::AddressCoordinate::new(KIND_LONG_FORM_ARTICLE, author, d_tag).to_wire()
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

/// A generic repost whose target is named by an `a` tag (proven coordinate),
/// with no embedded body and no local lookup.
fn address_repost(id: &str, author: &str, coord: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: nmp_nip18::KIND_GENERIC_REPOST,
        created_at,
        tags: vec![
            vec!["a".to_string(), coord.to_string()],
            vec!["k".to_string(), KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn delete_event(id: &str, author: &str, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: nmp_nip18::KIND_DELETE,
        created_at: 99,
        tags: tags
            .into_iter()
            .map(|t| t.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn primary_kind30023_acquires_kind16_and_deletes_not_kind6() {
    assert_eq!(
        longform_acquisition_kinds(),
        [
            KIND_LONG_FORM_ARTICLE,
            nmp_nip18::KIND_GENERIC_REPOST,
            nmp_nip18::KIND_DELETE,
        ]
        .into_iter()
        .collect(),
        "an addressable feed must acquire kind:16 reposts and kind:5 deletes, not kind:6"
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

#[test]
fn address_tag_repost_positions_coordinate_row_without_fetching_body() {
    // E02/E03: an `a`-tag repost proves the coordinate even with no embedded
    // body and no local lookup. The row exists keyed at the coordinate; the
    // article body is unresolved (None) but the identity is NOT guessed.
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|author| {
        author == AUTHOR_C
    })));
    let coord = address(AUTHOR_A, "article-a");

    feed.on_kernel_event(&address_repost("wrapper", AUTHOR_C, &coord, 40));

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1);
    let row = &snapshot.cards[0].card;
    assert_eq!(row.id, coord);
    assert!(
        row.article.is_none(),
        "body is unresolved; only the coordinate identity is proven"
    );
    assert_eq!(row.reposted_by.as_ref().unwrap().repost_event_id, "wrapper");
}

#[test]
fn address_repost_and_direct_article_collapse_to_one_coordinate_row() {
    // E02/E03 collapse: an `a`-tag repost and the real article at the same
    // coordinate are ONE row, not two; the body hydrates the existing row.
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    let coord = address(AUTHOR_A, "article-a");
    let real = article("real-id", AUTHOR_A, "article-a", 10, "nostr");

    feed.on_kernel_event(&address_repost("wrapper", AUTHOR_C, &coord, 40));
    feed.on_kernel_event(&real);

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1, "versions collapse to one row");
    let row = &snapshot.cards[0].card;
    assert_eq!(row.id, coord);
    assert_eq!(row.article.as_ref().unwrap().id, "real-id");
}

#[test]
fn newer_article_at_coordinate_collapses_older_version() {
    // E02/E03 latest-at-coordinate: a newer event at the same (pubkey,kind,d)
    // replaces the older; versions do not stack.
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    let v1 = article("v1", AUTHOR_A, "same", 10, "nostr");
    let v2 = article("v2", AUTHOR_A, "same", 50, "nostr");

    feed.on_kernel_event(&v1);
    feed.on_kernel_event(&v2);

    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1);
    assert_eq!(snapshot.cards[0].card.id, address(AUTHOR_A, "same"));
    assert_eq!(snapshot.cards[0].card.article.as_ref().unwrap().id, "v2");
}

#[test]
fn kind5_coordinate_delete_by_owner_removes_row() {
    // H06: a kind:5 with an `a` tag deletes the coordinate row — when the
    // delete author owns the coordinate.
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    let real = article("real-id", AUTHOR_A, "article-a", 10, "nostr");
    feed.on_kernel_event(&real);
    assert_eq!(feed.len(), 1);

    let coord = address(AUTHOR_A, "article-a");
    feed.on_kernel_event(&delete_event("del", AUTHOR_A, vec![vec!["a", &coord]]));

    assert!(feed.is_empty(), "owner's a-tag delete removes the row");
}

#[test]
fn kind5_coordinate_delete_by_foreign_author_is_noop() {
    // H06 negative: a foreign delete cannot remove someone else's coordinate.
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    let real = article("real-id", AUTHOR_A, "article-a", 10, "nostr");
    feed.on_kernel_event(&real);

    let coord = address(AUTHOR_A, "article-a");
    feed.on_kernel_event(&delete_event("del", AUTHOR_B, vec![vec!["a", &coord]]));

    assert_eq!(
        feed.len(),
        1,
        "foreign a-tag delete must not remove the row"
    );
}

#[test]
fn kind5_event_id_delete_removes_repost_source() {
    // A kind:5 with an `e` tag removes the row positioned by that event id —
    // here a repost wrapper, validated against its author.
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|author| {
        author == AUTHOR_C
    })));
    let target = article("target", AUTHOR_A, "article-a", 10, "nostr");
    feed.on_kernel_event(&embedded_repost("wrapper", AUTHOR_C, &target, 40));
    assert_eq!(feed.len(), 1);

    feed.on_kernel_event(&delete_event("del", AUTHOR_C, vec![vec!["e", "wrapper"]]));

    assert!(
        feed.is_empty(),
        "author's e-tag delete removes the source row"
    );
}

#[test]
fn kind5_coordinate_delete_does_not_remove_a_newer_version() {
    // A newer version published AFTER the deletion request survives: the store
    // (and feed) only retract versions created at or before the delete.
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    feed.on_kernel_event(&article("v1", AUTHOR_A, "article-a", 10, "nostr"));

    let coord = address(AUTHOR_A, "article-a");
    // delete_event timestamps at 99; a v2 at 200 is newer than the deletion.
    feed.on_kernel_event(&article("v2", AUTHOR_A, "article-a", 200, "nostr"));
    feed.on_kernel_event(&delete_event("del", AUTHOR_A, vec![vec!["a", &coord]]));

    assert_eq!(feed.len(), 1, "a version newer than the delete survives");
    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards[0].card.article.as_ref().unwrap().id, "v2");
}

#[test]
fn kind5_coordinate_delete_does_not_reanimate_an_older_retained_source() {
    // Regression: an `a`-tag delete must remove the retracted version at the
    // SOURCE level, not just drop the best row. Otherwise an older source (v1,
    // older than the delete) lingers in the row's sources and reanimates when
    // the surviving newer source (v2) is later removed.
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    let coord = address(AUTHOR_A, "article-a");
    let v1 = article("v1", AUTHOR_A, "article-a", 10, "nostr");
    let v2 = article("v2", AUTHOR_A, "article-a", 200, "nostr");

    // Both versions surface the coordinate: v1 via a repost source, v2 direct.
    feed.on_kernel_event(&embedded_repost("repost-of-v1", AUTHOR_C, &v1, 30));
    feed.on_kernel_event(&v2);
    assert_eq!(feed.len(), 1, "one coordinate row");

    // Delete at t=99 retracts v1 (<=99) but not v2 (>99). The row stays as v2.
    feed.on_kernel_event(&delete_event("del", AUTHOR_A, vec![vec!["a", &coord]]));
    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 1);
    assert_eq!(snapshot.cards[0].card.article.as_ref().unwrap().id, "v2");

    // Now delete v2 too (its event id). The row must vanish — the retracted v1
    // source must NOT reanimate.
    feed.on_kernel_event(&delete_event("del2", AUTHOR_A, vec![vec!["e", "v2"]]));
    assert!(
        feed.is_empty(),
        "retracted v1 must not reanimate after v2 is removed"
    );
}

#[test]
fn kind5_with_unresolvable_target_is_noop() {
    // H06 negative: a kind:5 whose only target cannot resolve to a row (an
    // `a` tag for a non-addressable kind, an `e` tag for nothing present)
    // changes nothing — never guess.
    let feed = LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)));
    let real = article("real-id", AUTHOR_A, "article-a", 10, "nostr");
    feed.on_kernel_event(&real);

    feed.on_kernel_event(&delete_event(
        "del",
        AUTHOR_A,
        vec![vec!["a", "1:aaaa:x"], vec!["e", "no-such-event"]],
    ));

    assert_eq!(feed.len(), 1, "unresolvable delete is a no-op");
}
