//! ADR-0045 E1 — aggregate-budget continuation + §6 watermark⇄serve
//! invariant tests (split from `cache_serve_tests.rs` for the 500-LOC
//! file ceiling; shared fixtures live there as `pub(super)` helpers).

use super::cache_serve::shape_to_store_queries;
use super::cache_serve_tests::{drain_cache_serves, hex_pk, seed_events, simulate_cold_restart};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::StoreQuery;
use std::collections::BTreeSet;

// ─── 3. Aggregate per-tick budget + chunked continuation ────────────────────

/// ADR §5 — the merge-blocking property from the review: the follow feed
/// registers ONE single-author interest PER follow, so the budget must hold
/// at the AGGREGATE level, not just per interest.
///
/// 250 followed authors × 2 stored events each (500 events total, distinct
/// ascending timestamps). After `sync_follow_feed_interests`:
///
/// - the first (synchronous) step has served at most the aggregate tick
///   budget (2 × visible window = 160), NOT all 500;
/// - the continuation queue is non-empty;
/// - repeated `run_cache_serve_step` calls (the actor-tick piggyback) each
///   serve ≤ the budget and drain the queue within a bounded step count;
/// - after the drain every stored event has been served and every interest's
///   completion key is recorded.
#[test]
fn e1_aggregate_budget_chunks_across_ticks_for_many_follow_interests() {
    const AUTHORS: usize = 250;
    const EVENTS_PER_AUTHOR: usize = 2;
    const TOTAL: usize = AUTHORS * EVENTS_PER_AUTHOR;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let tick_budget = kernel.visible_limit * 2;
    assert!(
        TOTAL > 2 * tick_budget,
        "fixture must exceed two tick budgets to prove multi-tick continuation"
    );

    let base_ts: u64 = 1_700_000_000;
    let mut author_keys: Vec<::nostr::Keys> = Vec::with_capacity(AUTHORS);
    let mut follows: Vec<String> = Vec::with_capacity(AUTHORS);
    for _ in 0..AUTHORS {
        let keys = ::nostr::Keys::generate();
        follows.push(keys.public_key().to_hex());
        author_keys.push(keys);
    }

    kernel.active_account = Some(hex_pk("aa"));
    kernel.follow_feed_kinds = BTreeSet::from([1u32]);

    // Seed the store: distinct ascending timestamps so every event can beat
    // the aggregate-window floor (over-serve is allowed; the assertion is the
    // per-step CAP, not a minimum).
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

    // The sync enqueues one serve per follow (+1 for the active account) and
    // synchronously drains exactly ONE aggregate-budget chunk.
    kernel.sync_follow_feed_interests(&follows);

    let after_first_step = kernel.events.len();
    assert!(
        after_first_step <= tick_budget,
        "first step must serve at most the aggregate tick budget \
         ({tick_budget}), got {after_first_step} — per-interest budgets alone \
         are the #1085 anti-pattern"
    );
    assert!(after_first_step > 0, "first step must serve something");
    assert!(
        kernel.has_pending_cache_serves(),
        "work beyond the first chunk must remain queued for the actor tick"
    );

    // Continuation: drain on subsequent "ticks" (each step ≤ budget, asserted
    // inside the helper). 500 events / 160 per tick → must finish well within
    // 20 steps.
    let steps = drain_cache_serves(&mut kernel, 20);
    assert!(steps >= 2, "drain must have required multiple continuation ticks");

    // Everything on disk was served (ascending timestamps all beat the floor).
    assert_eq!(
        kernel.events.len(),
        TOTAL,
        "after the drain every stored event must have been served"
    );
    // Every interest completed: follows + the active account.
    assert_eq!(
        kernel.served_interest_shapes.len(),
        AUTHORS + 1,
        "every per-follow interest (plus the active account's) must record \
         its completion key after the drain"
    );
    assert!(!kernel.has_pending_cache_serves(), "queue must be empty");
}

// ─── 5. Watermark ⇄ serve invariant (§6) ────────────────────────────────────

/// ADR-0045 §6: **no watermark floor without cache-serve coverage for the
/// same shape** — the load-bearing implication is `floored ⇒ served`,
/// asserted here against the REAL production `watermark_fn` installed by the
/// kernel constructor (not a re-derivation of its rules).
///
/// The kernel is seeded with real stored events for one author so the
/// watermark has something to floor — making the single-author case a
/// non-vacuous "floored AND served" probe. A tagged author+kind shape is the
/// regression direction: the watermark's per-author-newest scan ignores the
/// tag dimension, so flooring it would park stored tagged events above the
/// floor with no serve to back them — the watermark must refuse (`None`).
///
/// The structural variant mapping (which `StoreQuery` each E1 shape produces)
/// is pinned alongside.
#[test]
fn e1_watermark_serve_invariant_shapes_are_aligned() {
    use crate::planner::InterestShape;

    // A live kernel with stored events for `author` (kind 1).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    kernel.timeline_authors.insert(author.clone());
    seed_events(&mut kernel, &keys, 2, 1_700_000_000);

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
    let shape_no_kinds = InterestShape::default();

    // ── The load-bearing implication: floored ⇒ served ──────────────────────
    let cases: [(&str, &InterestShape); 6] = [
        ("single-author+kind", &shape_single_author),
        ("multi-author one-empty", &shape_author_no_events),
        ("kindtime", &shape_kindtime),
        ("tagged author+kind", &shape_tagged),
        ("event-ids", &shape_event_ids),
        ("no-kinds", &shape_no_kinds),
    ];
    for (name, shape) in cases {
        let floored = kernel
            .lifecycle
            .watermark_for_shape_for_test(shape)
            .is_some();
        let served = !shape_to_store_queries(shape).is_empty();
        assert!(
            !floored || served,
            "§6 violated for `{name}`: the watermark floors this shape but \
             cache-serve does not cover it — stored events above the floor \
             would never reach projections (relay won't resend, serve won't feed)"
        );
    }

    // Non-vacuity: the single-author shape with stored events IS floored
    // (so the implication above is exercised in the `floored == true` arm).
    assert!(
        kernel
            .lifecycle
            .watermark_for_shape_for_test(&shape_single_author)
            .is_some(),
        "single-author shape with stored events must be floored — otherwise \
         the floored⇒served assertion is vacuous"
    );
    // Regression direction: the tagged shape must NOT be floored, because E1
    // serve does not cover it (the superset author-newest floor would be
    // unsafe — see watermark_fn).
    assert!(
        kernel
            .lifecycle
            .watermark_for_shape_for_test(&shape_tagged)
            .is_none(),
        "tagged shapes must not be floored while cache-serve does not cover them"
    );

    // ── Structural variant mapping (E1 shape → StoreQuery) ─────────────────
    let queries = shape_to_store_queries(&shape_single_author);
    assert_eq!(queries.len(), 1, "1 author + 1 kind → 1 AuthorKind query");
    match &queries[0] {
        StoreQuery::AuthorKind { kinds, .. } => assert_eq!(kinds, &vec![1u32]),
        other => panic!("expected AuthorKind, got {other:?}"),
    }

    let queries2 = shape_to_store_queries(&shape_author_no_events);
    assert_eq!(queries2.len(), 2, "2 authors + 1 kind → 2 AuthorKind queries");
    assert!(queries2
        .iter()
        .all(|q| matches!(q, StoreQuery::AuthorKind { .. })));

    let queries3 = shape_to_store_queries(&shape_kindtime);
    assert_eq!(queries3.len(), 1, "0 authors + 1 kind → 1 KindTime query");
    assert!(matches!(&queries3[0], StoreQuery::KindTime { .. }));

    assert!(
        shape_to_store_queries(&shape_no_kinds).is_empty(),
        "0 kinds → no queries (not covered by E1)"
    );
    assert!(
        shape_to_store_queries(&shape_tagged).is_empty(),
        "tagged shapes → no queries (E2/E3)"
    );
    assert!(
        shape_to_store_queries(&shape_event_ids).is_empty(),
        "event-id shapes → no queries (E2/E3)"
    );
}
