//! Tests for [`super::FlatFeed`] — the predicate-gated flat note feed
//! (ADR-0042 §5.1, ADR-0058 §8 6B viewport grow).

use super::*;

fn ev(
    id: &str,
    author: &str,
    kind: u32,
    created_at: u64,
    tags: Vec<Vec<String>>,
) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at,
        tags,
        content: format!("note {id}"),
        relay_provenance: Vec::new(),
    }
}

fn etag(id: &str) -> Vec<String> {
    vec!["e".to_string(), id.to_string()]
}

#[test]
fn author_feed_admits_only_that_author_and_kinds() {
    let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
    feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
    feed.ingest(&ev("a2", "alice", 6, 101, vec![])); // repost — admitted
    feed.ingest(&ev("a3", "alice", 7, 102, vec![])); // reaction — rejected
    feed.ingest(&ev("b1", "bob", 1, 103, vec![])); // other author — rejected
    assert_eq!(feed.len(), 2);
    let snap = feed.snapshot(&FeedRequest::default());
    // Newest-first: a2 (101) before a1 (100).
    assert_eq!(snap.cards.len(), 2);
    assert_eq!(snap.cards[0].card.id, "a2");
    assert_eq!(snap.cards[1].card.id, "a1");
    // Flat feed never carries attribution.
    assert!(snap.cards.iter().all(|c| c.attribution.is_empty()));
}

#[test]
fn author_feed_includes_replies_to_others_as_top_level_rows() {
    // The exact case RootIndexedFeed cannot express: alice's reply to bob's
    // note is a top-level row in alice's profile, not attribution under bob.
    let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
    feed.ingest(&ev("reply", "alice", 1, 200, vec![etag("bobs_root")]));
    assert_eq!(feed.len(), 1);
    assert_eq!(
        feed.snapshot(&FeedRequest::default()).cards[0].card.id,
        "reply"
    );
}

#[test]
fn thread_feed_admits_root_by_id_and_referrers_by_etag() {
    let feed = FlatFeed::new(thread_feed_predicate("root".to_string(), vec![1, 6]));
    feed.ingest(&ev("root", "alice", 1, 100, vec![])); // root — by id
    feed.ingest(&ev("reply1", "bob", 1, 101, vec![etag("root")])); // referrer
    feed.ingest(&ev("reply2", "carol", 1, 102, vec![etag("other")])); // unrelated
    feed.ingest(&ev("react", "dave", 7, 103, vec![etag("root")])); // wrong kind
    assert_eq!(feed.len(), 2);
    let ids: Vec<_> = feed
        .snapshot(&FeedRequest::default())
        .cards
        .iter()
        .map(|c| c.card.id.clone())
        .collect();
    assert_eq!(ids, vec!["reply1".to_string(), "root".to_string()]);
}

#[test]
fn reingest_same_id_refreshes_not_duplicates() {
    let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
    feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
    feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
    assert_eq!(feed.len(), 1);
}

#[test]
fn snapshot_windows_to_request_limit_with_cursor() {
    let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
    for i in 0..5u64 {
        feed.ingest(&ev(&format!("a{i}"), "alice", 1, 100 + i, vec![]));
    }
    let snap = feed.snapshot(&FeedRequest::newest(2));
    assert_eq!(snap.cards.len(), 2);
    let page = snap.page.expect("page");
    assert!(page.has_more);
    assert_eq!(page.total_blocks, 5);
    assert!(page.next_cursor.is_some());
}

#[test]
fn on_kernel_event_observer_entrypoint_renders_matching_event() {
    // The load-bearing seam: in production the kernel admits an
    // open_interest-matched event into `self.events` and then calls
    // `notify_event_observers` → `FlatFeed::on_kernel_event` (NOT the
    // private `ingest`). `event_observer_tests.rs` proves the kernel fires
    // `on_kernel_event` once per accepted ingest; this proves the FlatFeed
    // observer entry point forwards through the predicate + render path, so
    // the full chain (admission → fan-out → snapshot) holds end-to-end.
    let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
    // Drive via the KernelEventObserver trait method, exactly as
    // `notify_event_observers` does.
    KernelEventObserver::on_kernel_event(&*feed, &ev("a1", "alice", 1, 100, vec![]));
    KernelEventObserver::on_kernel_event(&*feed, &ev("b1", "bob", 1, 101, vec![]));
    // Only alice's note rendered (predicate gate honoured at the observer
    // entry point), and it surfaces in the FlatFeed snapshot.
    assert_eq!(feed.len(), 1);
    let snap = feed.snapshot(&nmp_feed::FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card.id, "a1");
}

#[test]
fn bare_flat_feed_fails_closed_no_pull_interest() {
    // ADR-0058 §8 6B: a `FlatFeed::new` has no covered pull interest, so it
    // fails closed — a `PullFeedController` would refuse to construct and the
    // feed renders projection-only.
    let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
    assert!(feed.interest_shape().is_none(), "bare flat feed fails closed");
}

#[test]
fn author_feed_shape_is_a_covered_authors_kind_interest() {
    let shape = author_feed_shape("alice".to_string(), vec![1, 6]);
    assert_eq!(shape.authors, BTreeSet::from(["alice".to_string()]));
    assert_eq!(shape.kinds, BTreeSet::from([1, 6]));
    assert!(shape.tags.is_empty(), "author feed is authors+kinds only");
    // A feed built with_interest surfaces the shape to the pull controller.
    let feed = FlatFeed::with_interest(
        author_feed_predicate("alice".to_string(), vec![1, 6]),
        Some(shape.clone()),
    );
    assert_eq!(feed.interest_shape(), Some(shape));
}

#[test]
fn thread_feed_shape_is_a_covered_etag_reply_tail() {
    let shape = thread_feed_shape("root".to_string(), vec![1, 6]);
    assert_eq!(shape.kinds, BTreeSet::from([1, 6]));
    assert_eq!(
        shape.tags.get("e"),
        Some(&BTreeSet::from(["root".to_string()])),
        "thread shape pages the #e reply tail; the root rides the claim path"
    );
    assert!(shape.authors.is_empty());
    assert!(
        shape.event_ids.is_empty(),
        "root-by-id is event-id-only (uncovered) — never folded into the pull shape"
    );
}

#[test]
fn grow_visible_window_reveals_rows_past_the_default_first_page() {
    // BLOCKING 2: when a `load_older` pull ingests older rows, they sort
    // BELOW the newest-first first page. Without a growable viewport the
    // emitted snapshot stays capped at `DEFAULT_FEED_WINDOW_LIMIT`, so the
    // user never sees the pulled rows. `grow_visible_window` (the host's
    // `advance` step) must widen the EMITTED projection over already-
    // ingested rows.
    let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
    let extra = 25usize;
    let total = DEFAULT_FEED_WINDOW_LIMIT + extra;
    for i in 0..total as u64 {
        feed.ingest(&ev(&format!("a{i:04}"), "alice", 1, 100 + i, vec![]));
    }
    assert_eq!(feed.len(), total, "all rows ingested");

    // First page (the default sidecar window): exactly DEFAULT rows, with
    // more pending.
    let first = feed.snapshot_current_window();
    assert_eq!(
        first.cards.len(),
        DEFAULT_FEED_WINDOW_LIMIT,
        "default window emits only the first page"
    );
    assert!(first.page.expect("page").has_more, "more rows below the page");

    // The advance step after a drained page: the viewport grows and the
    // EMITTED projection now includes the previously-hidden older rows.
    assert!(feed.grow_visible_window(), "viewport grows (more rows to show)");
    let grown = feed.snapshot_current_window();
    assert_eq!(
        grown.cards.len(),
        total,
        "grown window emits the rows revealed past the first page"
    );
    // Order is still newest-first by (created_at, id): the freshly revealed
    // rows are the OLDEST, appended at the tail.
    assert_eq!(grown.cards[0].card.id, format!("a{:04}", total - 1));
    assert_eq!(grown.cards[total - 1].card.id, "a0000");

    // Idempotent at the ceiling-of-data: nothing more to reveal.
    assert!(
        !feed.grow_visible_window(),
        "no growth once every ingested row is visible"
    );
}

#[test]
fn feed_controller_emits_home_feed_wire_shape() {
    // The snapshot must produce the RootFeedSnapshot shape the home
    // feed emits, so the existing Swift `nmp.feed.home` reader decodes it.
    let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
    feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
    let snap = feed.snapshot(&nmp_feed::FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert!(snap.cards[0].attribution.is_empty());
}
