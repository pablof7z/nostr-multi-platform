//! Tests for [`super::FlatFeed`] — the predicate-gated flat note feed
//! (ADR-0076 §5.1, ADR-0072 §8 6B viewport grow).

use super::*;

use nmp_feed::DEFAULT_FEED_WINDOW_LIMIT;

fn ev(id: &str, author: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
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

fn social_acquisition_kinds() -> Vec<u32> {
    nmp_nip18::acquisition_kinds_for_primary([nmp_nip01::KIND_SHORT_TEXT_NOTE])
        .into_iter()
        .collect()
}

fn social_acquisition_set() -> BTreeSet<u32> {
    social_acquisition_kinds().into_iter().collect()
}

#[test]
fn author_feed_admits_only_that_author_and_kinds() {
    let feed = FlatFeed::new(author_feed_predicate(
        "alice".to_string(),
        social_acquisition_kinds(),
    ));
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
    let feed = FlatFeed::new(author_feed_predicate(
        "alice".to_string(),
        social_acquisition_kinds(),
    ));
    feed.ingest(&ev("reply", "alice", 1, 200, vec![etag("bobs_root")]));
    assert_eq!(feed.len(), 1);
    assert_eq!(
        feed.snapshot(&FeedRequest::default()).cards[0].card.id,
        "reply"
    );
}

#[test]
fn thread_feed_admits_root_by_id_and_referrers_by_etag() {
    let feed = FlatFeed::new(thread_feed_predicate(
        "root".to_string(),
        social_acquisition_kinds(),
    ));
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
fn repost_wrapper_and_target_share_canonical_feed_item() {
    let feed = FlatFeed::new(thread_feed_predicate(
        "root".to_string(),
        social_acquisition_kinds(),
    ));

    feed.ingest(&ev("repost", "alice", 6, 200, vec![etag("root")]));
    let placeholder = feed.snapshot(&FeedRequest::default());
    assert_eq!(placeholder.cards.len(), 1);
    assert_eq!(placeholder.cards[0].card.id, "root");
    assert_eq!(placeholder.cards[0].card.created_at, 200);
    assert_eq!(placeholder.cards[0].card.content, "");
    assert_eq!(
        placeholder.cards[0]
            .card
            .reposted_by
            .as_ref()
            .expect("repost attribution")
            .author_pubkey,
        "alice"
    );

    feed.ingest(&ev("root", "bob", 1, 100, vec![]));
    let hydrated = feed.snapshot(&FeedRequest::default());

    assert_eq!(hydrated.cards.len(), 1, "target id stays deduped");
    let card = &hydrated.cards[0].card;
    assert_eq!(card.id, "root");
    assert_eq!(card.author_pubkey, "bob");
    assert_eq!(card.content, "note root");
    assert_eq!(
        card.created_at, 200,
        "feed ordering keeps the repost wrapper timestamp"
    );
    let reposted_by = card.reposted_by.as_ref().expect("repost attribution");
    assert_eq!(reposted_by.author_pubkey, "alice");
    assert_eq!(
        reposted_by.note_created_at, 100,
        "render metadata still exposes the target event timestamp"
    );
}

/// Regression for issue #1496 — the real ThreadScreen crash path, end to end.
///
/// ## The crash
///
/// `ThreadScreen.swift:18` builds `Dictionary(uniqueKeysWithValues:)` over the
/// decoded `OpFeedSnapshot.cards`, keyed by `card.id`. It fatal-asserts when two
/// cards share a `card.id`. A real device crash report (issue #1496,
/// 2026-06-17) proves two cards with the same `card.id` reached this screen.
///
/// ## The true root cause (file:line)
///
/// `NoteFeedItem::from_event_for_op_feed` FORCES a kind:6 repost's `card.id` to the
/// **target** event id, so a repost-of-X and the original X both render
/// `card.id == X`. At crash time the per-app thread `FlatFeed::ingest` keyed its
/// row map by **`event.id`** (the kind:6 wrapper id vs the original id are
/// distinct), so the two contributions landed in *separate* rows whose cards
/// both carried `card.id == X` → the snapshot emitted two cards with one
/// `card.id` → the Swift dict asserted. PR #1636 re-keyed the (now shared,
/// generic) `nmp_feed::FlatFeed` by the **canonical item id** (`incoming.id`,
/// which the `event_card_builder` sets to `card.id`) with `source_id`
/// tracking, so the original row and the repost wrapper now collapse into one
/// canonical row.
///
/// This test pins that invariant against re-introduction of event-id keying by
/// driving the **exact production path** ThreadScreen reads: ingest the original
/// kind:1 root as its own row FIRST, then a kind:6 repost wrapper of it, then
/// `snapshot_current_window()` → `encode_op_feed_snapshot` →
/// `decode_op_feed_snapshot` (the wire round-trip the iOS shell decodes), and
/// asserts (a) exactly one card per `card.id` so the Swift dict cannot crash,
/// and (b) the **survivor's identity**: the canonical row carries the original
/// note's body and author (the hydrated target, not the placeholder wrapper),
/// with the repost provenance preserved. Under the crash-time `event.id` keying
/// this would emit two `card.id == "rootid"` cards and the unique-id assert
/// would fail.
#[test]
fn issue_1496_thread_root_then_repost_wire_path_has_unique_card_ids() {
    use crate::op_feed::{decode_op_feed_snapshot, encode_op_feed_snapshot};

    let root_id = "rootid";
    let feed = FlatFeed::new(thread_feed_predicate(
        root_id.to_string(),
        social_acquisition_kinds(),
    ));

    // Production order for the crash: the original note is already a thread row
    // (admitted by `event.id == root_id`) BEFORE the repost wrapper arrives.
    // This is the ordering the prior `repost_wrapper_and_target_share_canonical`
    // test did NOT cover (it ingested the repost first), and it is the ordering
    // under which event-id keying produced two rows.
    feed.ingest(&ev(root_id, "alice", 1, 1_000, vec![]));
    // A kind:6 repost of the root: its own (distinct) wrapper event id, an `#e`
    // tag pointing at the root, no embedded body. `from_event_for_op_feed`
    // forces this card's `id` to the target (`root_id`).
    feed.ingest(&ev("wrapper", "bob", 6, 2_000, vec![etag(root_id)]));

    // Drive the FULL wire path ThreadScreen decodes (not a hand-built snapshot).
    let snapshot = feed.snapshot_current_window();
    let bytes = encode_op_feed_snapshot(&snapshot);
    let decoded = decode_op_feed_snapshot(&bytes).expect("decode NNFS OpFeedSnapshot");

    // (a) Unique `card.id`: the Swift `Dictionary(uniqueKeysWithValues:)` cannot
    // crash. Under crash-time event.id keying this vector held two "rootid".
    let ids: Vec<&str> = decoded.cards.iter().map(|c| c.card.id.as_str()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        ids.len(),
        unique.len(),
        "issue #1496: decoded thread cards must have unique card.id; got {ids:?}"
    );
    assert_eq!(decoded.cards.len(), 1, "root + repost collapse to one row");

    // (b) Survivor identity — assert WHICH contribution won, not just the count.
    // The canonical row keeps the original (hydrated) note's id, author, and
    // body; the repost is surfaced via `reposted_by`, and the row sorts at the
    // repost timestamp.
    let card = &decoded.cards[0].card;
    assert_eq!(card.id, root_id, "survivor keeps the original note id");
    assert_eq!(
        card.author_pubkey, "alice",
        "survivor renders the ORIGINAL author, not the reposter"
    );
    assert_eq!(
        card.content, "note rootid",
        "survivor renders the ORIGINAL note body, not the empty repost placeholder"
    );
    let reposted_by = card
        .reposted_by
        .as_ref()
        .expect("repost provenance preserved on the merged row");
    assert_eq!(
        reposted_by.author_pubkey, "bob",
        "repost provenance names the wrapper author"
    );
}

#[test]
fn reingest_same_id_refreshes_not_duplicates() {
    let feed = FlatFeed::new(author_feed_predicate(
        "alice".to_string(),
        social_acquisition_kinds(),
    ));
    feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
    feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
    assert_eq!(feed.len(), 1);
}

#[test]
fn snapshot_windows_to_request_limit_with_cursor() {
    let feed = FlatFeed::new(author_feed_predicate(
        "alice".to_string(),
        social_acquisition_kinds(),
    ));
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
    let feed = FlatFeed::new(author_feed_predicate(
        "alice".to_string(),
        social_acquisition_kinds(),
    ));
    // Drive via the ObservedProjectionSink trait method, exactly as
    // `notify_event_observers` does.
    ObservedProjectionSink::on_kernel_event(&*feed, &ev("a1", "alice", 1, 100, vec![]));
    ObservedProjectionSink::on_kernel_event(&*feed, &ev("b1", "bob", 1, 101, vec![]));
    // Only alice's note rendered (predicate gate honoured at the observer
    // entry point), and it surfaces in the FlatFeed snapshot.
    assert_eq!(feed.len(), 1);
    let snap = feed.snapshot(&nmp_feed::FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card.id, "a1");
}

#[test]
fn bare_flat_feed_fails_closed_no_pull_interest() {
    // ADR-0072 §8 6B: a `FlatFeed::new` has no covered pull interest, so it
    // fails closed — a `PullFeedController` would refuse to construct and the
    // feed renders projection-only.
    let feed = FlatFeed::new(author_feed_predicate(
        "alice".to_string(),
        social_acquisition_kinds(),
    ));
    assert!(
        feed.interest_shape().is_none(),
        "bare flat feed fails closed"
    );
}

#[test]
fn author_feed_shape_is_a_covered_authors_kind_interest() {
    let shape = author_feed_shape("alice".to_string(), social_acquisition_kinds());
    assert_eq!(shape.authors, BTreeSet::from(["alice".to_string()]));
    assert_eq!(shape.kinds, social_acquisition_set());
    assert!(shape.tags.is_empty(), "author feed is authors+kinds only");
    // A feed built with_interest surfaces the shape to the pull controller.
    let feed = FlatFeed::with_interest(
        author_feed_predicate("alice".to_string(), social_acquisition_kinds()),
        Some(shape.clone()),
    );
    assert_eq!(feed.interest_shape(), Some(shape));
}

#[test]
fn thread_feed_shape_is_a_covered_etag_reply_tail() {
    let shape = thread_feed_shape("root".to_string(), social_acquisition_kinds());
    assert_eq!(shape.kinds, social_acquisition_set());
    assert_eq!(
        shape.tags.get("e"),
        Some(&BTreeSet::from(["root".to_string()])),
        "thread shape pages the #e reply tail; the root is a separate dependency"
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
    let feed = FlatFeed::new(author_feed_predicate(
        "alice".to_string(),
        social_acquisition_kinds(),
    ));
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
    assert!(
        first.page.expect("page").has_more,
        "more rows below the page"
    );

    // The advance step after a drained page: the viewport grows and the
    // EMITTED projection now includes the previously-hidden older rows.
    assert!(
        feed.grow_visible_window(),
        "viewport grows (more rows to show)"
    );
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
fn feed_controller_emits_op_feed_wire_shape() {
    // The snapshot must produce the RootFeedSnapshot shape the OP feed emits,
    // so app-owned feed readers decode it through the note-feed typed sidecar.
    let feed = FlatFeed::new(author_feed_predicate(
        "alice".to_string(),
        social_acquisition_kinds(),
    ));
    feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
    let snap = feed.snapshot(&nmp_feed::FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert!(snap.cards[0].attribution.is_empty());
}
