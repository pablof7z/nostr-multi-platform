//! ADR-0045 E1 — aggregate-budget continuation + §6 watermark⇄serve
//! invariant tests (split from `cache_serve_tests.rs` for the 500-LOC
//! file ceiling; shared fixtures live there as `pub(super)` helpers).

use super::cache_serve::shape_to_store_queries;
use super::cache_serve_tests::{
    drain_cache_serves, hex_pk, open_author_interest, seed_events, simulate_cold_restart,
};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::StoreQuery;
use std::collections::BTreeSet;

// ─── 3. Collapsed follow-feed serve: ONE AuthorsKind, multi-author, bounded ──

/// #1497 test (b) — the follow-feed collapsed to ONE multi-author interest, so
/// its cache-serve issues a SINGLE `StoreQuery::AuthorsKind` (not a per-author
/// fan-out), serves the visible window newest-first across multiple followed
/// authors, and records exactly one completion key.
///
/// 250 authors × 2 stored events each (500 events total, distinct ascending
/// timestamps). After opening one reduced multi-author interest:
///
/// - the collapsed shape maps to exactly ONE `AuthorsKind` query;
/// - the synchronous drain serves the bounded visible window (≤ the tick
///   budget) — the 300–500-follow cold start is ONE multi-author scan, not 250
///   per-author serves (the ADR §5 anti-burst property, now intrinsic to the
///   collapse rather than enforced across N pending serves);
/// - the served timeline carries events from MULTIPLE distinct authors,
///   newest-first;
/// - exactly one completion key is recorded and the queue drains empty.
#[test]
fn e1_follow_feed_serves_single_authorskind_multi_author_newest_first() {
    use crate::planner::InterestShape;
    const AUTHORS: usize = 250;
    const EVENTS_PER_AUTHOR: usize = 2;
    const TOTAL: usize = AUTHORS * EVENTS_PER_AUTHOR;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let tick_budget = kernel.visible_limit * 2;

    let base_ts: u64 = 1_700_000_000;
    let mut author_keys: Vec<::nostr::Keys> = Vec::with_capacity(AUTHORS);
    let mut follows: Vec<String> = Vec::with_capacity(AUTHORS);
    for _ in 0..AUTHORS {
        let keys = ::nostr::Keys::generate();
        follows.push(keys.public_key().to_hex());
        author_keys.push(keys);
    }

    kernel.active_account = Some(hex_pk("aa"));

    // The collapsed multi-author shape maps to exactly ONE AuthorsKind query.
    let collapsed_shape = InterestShape {
        authors: follows.iter().cloned().collect(),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    let queries = shape_to_store_queries(&collapsed_shape);
    assert_eq!(queries.len(), 1, "collapsed follow-feed → ONE store query");
    assert!(
        matches!(&queries[0], StoreQuery::AuthorsKind { .. }),
        "collapsed follow-feed must map to a single AuthorsKind, got {:?}",
        queries[0]
    );

    // Seed the store: distinct ascending timestamps so the newest window is
    // unambiguous (the newest events span multiple authors).
    for (i, keys) in author_keys.iter().enumerate() {
        kernel.timeline_authors.insert(follows[i].clone());
        seed_events(
            &mut kernel,
            keys,
            EVENTS_PER_AUTHOR,
            base_ts + (i * EVENTS_PER_AUTHOR) as u64,
        );
    }
    assert_eq!(kernel.events.len(), TOTAL);

    simulate_cold_restart(&mut kernel);

    // The open enqueues ONE serve for the multi-author interest and
    // synchronously drains it. The serve is bounded by the visible window.
    open_author_interest(&mut kernel, "e1-collapsed", follows.clone(), [1u32]);

    let served = kernel.events.len();
    assert!(
        served > 0,
        "the synchronous drain must serve the visible window"
    );
    assert!(
        served <= tick_budget,
        "the collapsed serve is bounded by the tick budget ({tick_budget}), \
         got {served} — the 300–500-follow cold start is one bounded scan"
    );

    // Drain any continuation (none expected — depth ≤ visible window < budget).
    let _ = drain_cache_serves(&mut kernel, 20);

    // Exactly one completion key — ONE interest, ONE AuthorsKind query.
    assert_eq!(
        kernel.served_interest_shapes.len(),
        1,
        "the one multi-author follow-feed interest records exactly one \
         completion key"
    );
    assert!(!kernel.has_pending_cache_serves(), "queue must be empty");

    // The served timeline is newest-first across MULTIPLE distinct authors.
    let timeline_authors: Vec<String> = kernel
        .timeline
        .iter()
        .filter_map(|id| kernel.events.get(id).map(|e| e.author.clone()))
        .collect();
    assert!(
        !timeline_authors.is_empty(),
        "the served events must populate the timeline"
    );
    let distinct: BTreeSet<&String> = timeline_authors.iter().collect();
    assert!(
        distinct.len() > 1,
        "the collapsed serve must surface events from MULTIPLE followed \
         authors (got {} distinct in {} timeline items)",
        distinct.len(),
        timeline_authors.len()
    );
    // Newest-first: created_at is non-increasing down the timeline.
    let created: Vec<u64> = kernel
        .timeline
        .iter()
        .filter_map(|id| kernel.events.get(id).map(|e| e.created_at))
        .collect();
    assert!(
        created.windows(2).all(|w| w[0] >= w[1]),
        "the served timeline must be newest-first; got {created:?}"
    );
}

// ─── 5. Watermark ⇄ serve invariant (§6) ────────────────────────────────────

/// ADR-0045 §6 — the E1 shape→`StoreQuery` mapping (`shape_to_store_queries`):
/// which store query each author+kind / kindtime / event-id shape produces.
/// This is the single serve/pin mapping the floor-coherent pin set and
/// cache-serve both ride on; pinning the variant mapping guards it from drift.
#[test]
fn e1_watermark_serve_invariant_shapes_are_aligned() {
    use crate::planner::InterestShape;

    let author = hex_pk("a1");
    let shape_single_author = InterestShape {
        authors: BTreeSet::from([author.clone()]),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    let shape_author_no_events = InterestShape {
        authors: BTreeSet::from([author.clone(), hex_pk("ee")]),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    let shape_kindtime = InterestShape {
        kinds: BTreeSet::from([30023u32]),
        ..Default::default()
    };
    let shape_search = InterestShape {
        kinds: BTreeSet::from([1u32]),
        search: Some("nostr rust".to_string()),
        ..Default::default()
    };
    let mut shape_tagged = InterestShape {
        authors: BTreeSet::from([author.clone()]),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    shape_tagged
        .tags
        .insert("e".to_string(), BTreeSet::from(["abc".to_string()]));
    let mut shape_event_ids = InterestShape {
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    shape_event_ids.event_ids.insert(hex_pk("1d"));

    // The shape_tagged uses "abc" as the #e target (3 chars, not 64-char hex):
    // hex decode fails → no queries.
    assert!(
        shape_to_store_queries(&shape_tagged).is_empty(),
        "shape_tagged with 3-char target → no queries (hex decode fails)"
    );

    // ── Structural variant mapping (E1 shape → StoreQuery) ─────────────────
    let queries = shape_to_store_queries(&shape_single_author);
    assert_eq!(queries.len(), 1, "1 author + 1 kind → 1 AuthorKind query");
    match &queries[0] {
        StoreQuery::AuthorKind { kinds, .. } => assert_eq!(kinds, &vec![1u32]),
        other => panic!("expected AuthorKind, got {other:?}"),
    }

    // Multi-author shapes collapse to ONE `AuthorsKind` query (#1497 follow-feed
    // collapse), not a per-author `AuthorKind` fan-out.
    let queries2 = shape_to_store_queries(&shape_author_no_events);
    assert_eq!(
        queries2.len(),
        1,
        "2 authors + 1 kind → 1 AuthorsKind query (multi-author collapse)"
    );
    match &queries2[0] {
        StoreQuery::AuthorsKind { authors, kinds, .. } => {
            assert_eq!(authors.len(), 2, "AuthorsKind must carry both authors");
            assert_eq!(kinds, &vec![1u32]);
        }
        other => panic!("expected AuthorsKind, got {other:?}"),
    }

    let queries3 = shape_to_store_queries(&shape_kindtime);
    assert_eq!(queries3.len(), 1, "0 authors + 1 kind → 1 KindTime query");
    assert!(matches!(&queries3[0], StoreQuery::KindTime { .. }));

    assert!(
        shape_to_store_queries(&InterestShape::default()).is_empty(),
        "0 kinds → no queries (not covered by any increment)"
    );
    assert!(
        shape_to_store_queries(&shape_event_ids).is_empty(),
        "event-id shapes → no queries (not covered)"
    );
    assert!(
        shape_to_store_queries(&shape_search).is_empty(),
        "search shapes → no local StoreQuery; relay NIP-50 serves them"
    );
}

// ─── E3. Structural floored⇒served guard ────────────────────────────────────

/// ADR-0045 §6 — E3 extension: the floored⇒served invariant now holds for
/// Etag (threads), Ptag (DM inbox / mentions), and KindDtag (addressable) as
/// well as the E1 author+kind shapes.
///
/// This test uses properly 64-char-hex targets so that `hex_to_pubkey_bytes`
/// succeeds and real `StoreQuery` variants are produced. It asserts that every
/// E2/E3 shape produces a non-empty `shape_to_store_queries` result and pins the
/// variant mapping — the single serve/pin mapping seam the ADR §6 demands.
#[test]
fn e3_structural_floored_implies_served() {
    use crate::planner::{InterestShape, NaddrCoord};

    let author = hex_pk("a3");
    // 64-char event id for Etag target
    let event_id_hex = hex_pk("e1");
    // 64-char pubkey for Ptag target
    let ptag_hex = hex_pk("fa");

    // ── E2/E3: #p tag + kind:1059 (DM inbox) ────────────────────────────────
    let mut shape_dm_inbox = InterestShape {
        kinds: BTreeSet::from([1059u32]),
        ..Default::default()
    };
    shape_dm_inbox
        .tags
        .insert("p".to_string(), BTreeSet::from([ptag_hex.clone()]));

    // ── E3: #p tag + kind:9735 (mention/zap) ─────────────────────────────────
    let mut shape_ptag_mention = InterestShape {
        kinds: BTreeSet::from([9735u32]),
        ..Default::default()
    };
    shape_ptag_mention
        .tags
        .insert("p".to_string(), BTreeSet::from([ptag_hex.clone()]));

    // ── E3: #e tag + kind:1 (thread reply) ───────────────────────────────────
    let mut shape_etag_thread = InterestShape {
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    shape_etag_thread
        .tags
        .insert("e".to_string(), BTreeSet::from([event_id_hex.clone()]));

    // ── E3: addressable (kind:30023 long-form) ────────────────────────────────
    let mut shape_address = InterestShape {
        kinds: BTreeSet::from([30023u32]),
        ..Default::default()
    };
    shape_address.addresses.insert(NaddrCoord {
        pubkey: author.clone(),
        kind: 30023,
        d_tag: "my-article".to_string(),
    });

    // Every E2/E3 shape MUST produce a non-empty StoreQuery list (serve covers
    // it) — the structural property the floor-coherent pin set relies on.
    let cases: [(&str, &InterestShape); 4] = [
        ("DM inbox (#p+1059)", &shape_dm_inbox),
        ("#p mention/zap", &shape_ptag_mention),
        ("#e thread reply", &shape_etag_thread),
        ("addressable long-form", &shape_address),
    ];
    for (name, shape) in &cases {
        assert!(
            !shape_to_store_queries(shape).is_empty(),
            "E3 shape `{name}` must produce a non-empty StoreQuery list"
        );
    }

    // ── Structural variant mapping (E2/E3 shapes → StoreQuery) ──────────────
    let dm_queries = shape_to_store_queries(&shape_dm_inbox);
    assert_eq!(dm_queries.len(), 1, "DM inbox shape → 1 Ptag query");
    assert!(matches!(&dm_queries[0], StoreQuery::Ptag { .. }));

    let mention_queries = shape_to_store_queries(&shape_ptag_mention);
    assert_eq!(mention_queries.len(), 1, "#p mention → 1 Ptag query");
    assert!(matches!(&mention_queries[0], StoreQuery::Ptag { .. }));

    let thread_queries = shape_to_store_queries(&shape_etag_thread);
    assert_eq!(thread_queries.len(), 1, "#e thread → 1 Etag query");
    assert!(matches!(&thread_queries[0], StoreQuery::Etag { .. }));

    let addr_queries = shape_to_store_queries(&shape_address);
    assert_eq!(addr_queries.len(), 1, "addressable → 1 KindDtag query");
    assert!(matches!(&addr_queries[0], StoreQuery::KindDtag { .. }));

    // Multi-tag / multi-value shapes remain uncovered (refused by
    // shape_to_store_queries — the relay delivers in full for these).
    let mut shape_multi_tag = InterestShape {
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    shape_multi_tag
        .tags
        .insert("e".to_string(), BTreeSet::from([event_id_hex.clone()]));
    shape_multi_tag
        .tags
        .insert("p".to_string(), BTreeSet::from([ptag_hex.clone()]));
    assert!(
        shape_to_store_queries(&shape_multi_tag).is_empty(),
        "multi-tag shape → no queries (not covered)"
    );

    // event_ids still uncovered.
    let mut shape_event_ids2 = InterestShape {
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    shape_event_ids2.event_ids.insert(event_id_hex);
    assert!(
        shape_to_store_queries(&shape_event_ids2).is_empty(),
        "event-id shapes → no queries (not covered)"
    );
}
