//! `compile_default_lanes` proofs (#3092) — the collapsed single-lane
//! `FeedParams` path over the composite engine. These are the regression
//! harness for the deleted `nmp-note-feed` crate's `feed_row_builder`/
//! `timeline_merge` behavior: same row identity, same repost collapse, same
//! bump/prefer-hydrated merge outcome.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::ObservedProjectionSink;
use nmp_feed::{FeedRequest, FeedRowContext, FlatFeed};

use super::compile_default_lanes;

const FOLLOWED_AUTHOR: &str = "followed-author";
const OTHER_FOLLOWED_AUTHOR: &str = "other-followed-author";
const OUTSIDE_AUTHOR: &str = "outside-author";

fn note(author: &str, id: &str, created_at: u64, content: &str) -> KernelEvent {
    KernelEvent {
        id: EventId::from(id),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: Vec::new(),
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

fn embedded_repost(author: &str, id: &str, target: &KernelEvent, created_at: u64) -> KernelEvent {
    let content = format!(
        r#"{{"id":"{}","pubkey":"{}","kind":{},"created_at":{},"tags":[],"content":"{}"}}"#,
        target.id, target.author, target.kind, target.created_at, target.content
    );
    KernelEvent {
        id: EventId::from(id),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: vec![vec!["e".to_string(), target.id.clone()]],
        content,
        relay_provenance: Vec::new(),
    }
}

fn bare_repost(author: &str, id: &str, target_id: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: EventId::from(id),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: vec![vec!["e".to_string(), target_id.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn followed_admission() -> nmp_feed::RootAdmission {
    let followed: BTreeSet<String> = [FOLLOWED_AUTHOR.to_string(), OTHER_FOLLOWED_AUTHOR.to_string()]
        .into_iter()
        .collect();
    Arc::new(move |event: &KernelEvent| followed.contains(&event.author))
}

fn build_feed() -> Arc<FlatFeed<nmp_feed::FeedRow>> {
    let (item_builder, merge) = compile_default_lanes(
        followed_admission(),
        crate::source::empty_row_context(),
        &BTreeSet::from([1]),
        &BTreeSet::from([1, 6, 5]),
    );
    FlatFeed::with_merge(followed_admission(), item_builder, None, merge)
}

#[test]
fn a_direct_note_is_its_own_row() {
    let feed = build_feed();
    let event = note(FOLLOWED_AUTHOR, "n1", 100, "hello");
    feed.on_kernel_event(&event);

    let snapshot = feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(snapshot.cards.len(), 1);
    assert_eq!(snapshot.cards[0].card.canonical_row_id, "n1");
    assert_eq!(snapshot.cards[0].card.context, vec![FeedRowContext::Authored]);
}

#[test]
fn an_event_outside_the_admission_predicate_is_dropped() {
    let feed = build_feed();
    feed.on_kernel_event(&note(OUTSIDE_AUTHOR, "n1", 100, "hello"));
    assert!(feed.is_empty());
}

#[test]
fn an_embedded_repost_hydrates_immediately_and_dedupes_onto_the_target_id() {
    let feed = build_feed();
    // The reposted TARGET's author is NOT independently followed — its content
    // reaches the feed ONLY through the wrapper's embedded JSON, never as its
    // own admitted delivery.
    let target = note(OUTSIDE_AUTHOR, "target", 100, "outside content");
    let repost = embedded_repost(FOLLOWED_AUTHOR, "wrapper", &target, 200);
    feed.on_kernel_event(&repost);

    // A followed-author direct note authored at 150 — BETWEEN the target's own
    // authored time (100) and the repost wrapper's time (200). It pins the
    // repost row's SORT position: the repost row must sort ABOVE this note
    // (proving the row's sort_created_at is the repost bump 200, not the
    // target's 100), while the row's own `created_at` field shows the target's
    // real authored time 100.
    feed.on_kernel_event(&note(FOLLOWED_AUTHOR, "between", 150, "between content"));

    let snapshot = feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(
        snapshot.cards.len(),
        2,
        "repost dedups onto its target row; the between note is its own row"
    );
    let row = &snapshot.cards[0].card;
    assert_eq!(row.canonical_row_id, "target");
    assert_eq!(row.author_pubkey, OUTSIDE_AUTHOR);
    assert_eq!(row.content, "outside content");
    // #3092 ACCEPTED divergence (a): the card shows the reposted note's REAL
    // authored time (100), not the wrapper's time (200). The deleted single-lane
    // builder set the card `created_at` to the wrapper's time; the composite
    // engine hydrates it from the embedded event. Sort position stays bumped.
    assert_eq!(
        row.created_at, 100,
        "card created_at is the target's real authored time, not the wrapper's"
    );
    assert!(row.context.iter().any(|ctx| matches!(
        ctx,
        FeedRowContext::RepostedBy { author_pubkey, .. } if author_pubkey == FOLLOWED_AUTHOR
    )));
    // Sort position is the repost bump (200 > the between note's 150), NOT the
    // target's own 100 (which would sort the repost row BELOW the between note).
    assert_eq!(
        snapshot.cards[1].card.canonical_row_id, "between",
        "the repost row sorts above the between note ⇒ its sort position is the 200 bump, not 100"
    );
}

#[test]
fn a_bare_repost_of_an_also_followed_author_merges_in_the_real_content_and_keeps_the_bump() {
    let feed = build_feed();
    let target = note(OTHER_FOLLOWED_AUTHOR, "target", 100, "real content");
    let repost = bare_repost(FOLLOWED_AUTHOR, "wrapper", "target", 300);
    // A third, unrelated direct note sorts BETWEEN the target's own
    // `created_at` (100) and the repost's bump time (300) — this is what
    // proves the merged row's SORT POSITION used the bump (300), not the
    // target's own `created_at` (100): if the bump were lost, this note
    // would sort above the merged row instead of below it.
    let between = note(FOLLOWED_AUTHOR, "between", 200, "between content");

    // Repost arrives first (placeholder), target arrives second — the merge
    // must still adopt the target's real content while keeping the repost's
    // BUMPED sort position (300, not the target's own 100).
    feed.on_kernel_event(&repost);
    feed.on_kernel_event(&target);
    feed.on_kernel_event(&between);

    let snapshot = feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(snapshot.cards.len(), 2, "repost and target collapse to one row");
    let row = &snapshot.cards[0].card;
    assert_eq!(
        row.canonical_row_id, "target",
        "the bumped row sorts above the between note"
    );
    assert_eq!(row.author_pubkey, OTHER_FOLLOWED_AUTHOR);
    assert_eq!(row.content, "real content", "hydrated payload wins over the placeholder");
    assert!(row.context.iter().any(|ctx| matches!(ctx, FeedRowContext::Authored)));
    assert!(row.context.iter().any(|ctx| matches!(
        ctx,
        FeedRowContext::RepostedBy { author_pubkey, .. } if author_pubkey == FOLLOWED_AUTHOR
    )));
    assert_eq!(snapshot.cards[1].card.canonical_row_id, "between");
}

#[test]
fn a_repost_and_its_target_arriving_in_the_opposite_order_produce_the_same_row() {
    let feed = build_feed();
    let target = note(OTHER_FOLLOWED_AUTHOR, "target", 100, "real content");
    let repost = bare_repost(FOLLOWED_AUTHOR, "wrapper", "target", 300);

    feed.on_kernel_event(&target);
    feed.on_kernel_event(&repost);

    let snapshot = feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(snapshot.cards.len(), 1);
    let row = &snapshot.cards[0].card;
    assert_eq!(row.content, "real content");
}

/// A feed whose primary kind is an addressable content kind (kind:30023),
/// which derives the kind:16 generic-repost wrapper into the acquisition set.
fn build_addressable_feed() -> Arc<FlatFeed<nmp_feed::FeedRow>> {
    let (item_builder, merge) = compile_default_lanes(
        followed_admission(),
        crate::source::empty_row_context(),
        &BTreeSet::from([30_023]),
        &BTreeSet::from([30_023, 16, 5]),
    );
    FlatFeed::with_merge(followed_admission(), item_builder, None, merge)
}

fn addressable_repost(author: &str, id: &str, coord: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: EventId::from(id),
        author: author.to_string(),
        kind: 16,
        created_at,
        tags: vec![
            vec!["a".to_string(), coord.to_string()],
            vec!["k".to_string(), "30023".to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn an_addressable_repost_keys_by_its_target_coordinate_not_the_wrapper_id() {
    // #3092 ACCEPTED divergence (c): a kind:16 repost carrying an `a`-tag
    // coordinate keys its row by the TARGET's coordinate (kind:pubkey:d), so an
    // addressable article and its reposts collapse onto ONE coordinate row —
    // rather than the wrapper's own event id (which would never dedupe).
    let feed = build_addressable_feed();
    let coord = "30023:article-author:my-article";

    feed.on_kernel_event(&addressable_repost(FOLLOWED_AUTHOR, "wrapper-1", coord, 200));
    feed.on_kernel_event(&addressable_repost(OTHER_FOLLOWED_AUTHOR, "wrapper-2", coord, 250));

    let snapshot = feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(
        snapshot.cards.len(),
        1,
        "two reposts of the same coordinate collapse to one coordinate-keyed row"
    );
    assert_eq!(
        snapshot.cards[0].card.canonical_row_id, coord,
        "row is keyed by the target coordinate, not either wrapper's event id"
    );
}
