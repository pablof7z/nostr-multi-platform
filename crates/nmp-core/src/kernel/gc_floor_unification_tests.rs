//! K3 Stage C (ADR-0056 §3) — floor⇄serve predicate unification lock.
//!
//! `shape_floor` (the eviction-pin floor, `ram_eviction_floor.rs`) MUST read the
//! SAME `shape_to_store_queries` mapping the live `watermark_fn` reads (which
//! folds via `watermark_from_queries`). Before Stage C, `shape_floor` was a
//! hand-rolled SECOND shape→`StoreQuery` copy that had ALREADY drifted on the
//! addressable branch: it folded multi-coord `KindDtag` with MAX-ignoring-empties,
//! while `watermark_from_queries` uses the K3 Stage B1 MIN/abort-on-empty rule.
//!
//! These tests lock the unification: `shape_floor(shape, store, &truncated)` must
//! equal `watermark_from_queries(shape, <same store scan>, <same truncated>)` for
//! a battery of representative shapes, so the floor can never floor on a shape (or
//! at a timestamp) the serve mapping would not. This is the precondition that
//! makes the Stage D ledger swap a single-mapping migration.
//!
//! Split from `gc_floor_coherent_tests.rs` to keep both under the 500-LOC hard
//! cap (AGENTS.md file-size rule).

use super::super::ram_eviction_tests::{make_pubkey, pin_clock, T0_SECS};
use super::super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};

/// Ingest one kind:1 note through the real pre-verified ingest path so it lands
/// in both the RAM `events` map and the authoritative `self.store`.
fn inject_note(kernel: &mut Kernel, id: &str, pubkey: &str, created_at: u64) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: format!("note {id}"),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        crate::relay::RelayRole::Content,
        "",
        VerifiedEvent::from_raw_unchecked(raw),
    );
}

/// Ingest one kind:1 note tagged with an `e` reference (for Etag queries).
fn inject_note_with_etag(
    kernel: &mut Kernel,
    id: &str,
    pubkey: &str,
    created_at: u64,
    etag_target: &str,
) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 1,
        tags: vec![vec!["e".to_string(), etag_target.to_string()]],
        content: format!("note {id}"),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        crate::relay::RelayRole::Content,
        "",
        VerifiedEvent::from_raw_unchecked(raw),
    );
}

/// Ingest one addressable (kind:30023) event carrying a `d` tag, so it lands in
/// the store's `KindDtag` index.
fn inject_addressable(kernel: &mut Kernel, id: &str, pubkey: &str, created_at: u64, d_tag: &str) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 30023,
        tags: vec![vec!["d".to_string(), d_tag.to_string()]],
        content: format!("article {id}"),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        crate::relay::RelayRole::Content,
        "",
        VerifiedEvent::from_raw_unchecked(raw),
    );
}

/// Probe the kernel store for the newest stored `created_at` matching `q`,
/// normalizing `since`/`until` to `None` — exactly the scan the production
/// `watermark_fn` closure (`kernel/mod.rs`) installs.
fn watermark_store_scan(
    store: &dyn crate::store::EventStore,
    q: &crate::store::StoreQuery,
) -> Option<u64> {
    use crate::kernel::cache_serve::{query_since_mut, query_until_mut};
    let mut q = q.clone();
    if let Some(since) = query_since_mut(&mut q) {
        *since = None;
    }
    if let Some(until) = query_until_mut(&mut q) {
        *until = None;
    }
    let mut ts: Option<u64> = None;
    let _ = store.query_visit(&q, 1, &mut |ev| {
        ts = Some(ev.raw.created_at);
        std::ops::ControlFlow::Break(())
    });
    ts
}

/// The unified floor the live `watermark_fn` would install for `shape`, computed
/// directly over `shape_to_store_queries` via `watermark_from_queries` and the
/// real store scan (no truncated serves).
fn unified_floor(
    shape: &crate::planner::InterestShape,
    store: &dyn crate::store::EventStore,
) -> Option<u64> {
    use crate::kernel::cache_serve::watermark_from_queries;
    watermark_from_queries(shape, |q| watermark_store_scan(store, q), |_key| false)
}

/// K3 Stage C ORACLE — `shape_floor` agrees with `watermark_from_queries` for a
/// multi-coord addressable shape where ONE coordinate has stored events and the
/// other does not.
///
/// The unified predicate aborts (no floor) because an unfetched coord must
/// backfill in full (Stage B1). A residual `shape_floor` copy that folds
/// KindDtag with max-ignoring-empties returns `Some(stored coord newest)` —
/// flooring above the unfetched coord's never-fetched history.
///
/// FAILS on pre-Stage-C `shape_floor` (returns `Some`), passes once `shape_floor`
/// routes through `watermark_from_queries` (returns `None`).
#[test]
fn shape_floor_matches_unified_floor_for_partial_addressable_shape() {
    use crate::planner::{InterestShape, NaddrCoord};

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    let author = make_pubkey(9_001);
    // Coord A ("stored") has an event; coord B ("unfetched") has none.
    inject_addressable(
        &mut kernel,
        &format!("{:0>64x}", 0xAD0001u64),
        &author,
        500,
        "stored",
    );

    let mut shape = InterestShape {
        kinds: std::collections::BTreeSet::from([30023u32]),
        ..Default::default()
    };
    shape.addresses.insert(NaddrCoord {
        pubkey: author.clone(),
        kind: 30023,
        d_tag: "stored".to_string(),
    });
    shape.addresses.insert(NaddrCoord {
        pubkey: author.clone(),
        kind: 30023,
        d_tag: "unfetched".to_string(),
    });

    let truncated = std::collections::HashSet::new();
    let floor = super::floor::shape_floor(&shape, kernel.store.as_ref(), &truncated);
    let unified = unified_floor(&shape, kernel.store.as_ref());

    assert_eq!(
        unified, None,
        "sanity: the unified predicate aborts the floor for a partially-known \
         addressable shape (Stage B1 min/abort)"
    );
    assert_eq!(
        floor, unified,
        "K3 Stage C: shape_floor must equal the unified watermark_from_queries \
         floor — a partially-known addressable shape must NOT be floored (the \
         residual max-ignoring-empties copy floored at the stored coord's newest)"
    );
}

/// K3 Stage C LOCK — `shape_floor` is byte-identical to the unified
/// `watermark_from_queries` floor across a battery of representative shapes
/// (author+kind, multi-author one-empty, all-coords-stored addressable, #e
/// thread, global kind-only, uncovered no-kind). This is the standing guard that
/// no future edit can reintroduce a divergent second mapping: if the two ever
/// disagree for any shape in the battery, the test fails.
#[test]
fn shape_floor_equals_unified_floor_for_shape_battery() {
    use crate::planner::{InterestShape, NaddrCoord};
    use std::collections::{BTreeSet, HashSet};

    let mut kernel = Kernel::with_storage_path(DEFAULT_VISIBLE_LIMIT, None);
    pin_clock(&mut kernel, T0_SECS + 10_000);

    // Seed stored events so floors actually fire (non-vacuous battery).
    let author_a = make_pubkey(9_101);
    let author_b = make_pubkey(9_202);
    inject_note(
        &mut kernel,
        &format!("{:0>64x}", 0xBA0001u64),
        &author_a,
        100,
    );
    inject_note(
        &mut kernel,
        &format!("{:0>64x}", 0xBA0002u64),
        &author_a,
        300,
    );
    inject_note(
        &mut kernel,
        &format!("{:0>64x}", 0xBA0003u64),
        &author_b,
        200,
    );

    let etag_target = format!("{:0>64x}", 0xE7E7E7u64);
    inject_note_with_etag(
        &mut kernel,
        &format!("{:0>64x}", 0xCE0001u64),
        &author_a,
        150,
        &etag_target,
    );
    inject_note_with_etag(
        &mut kernel,
        &format!("{:0>64x}", 0xCE0002u64),
        &author_a,
        250,
        &etag_target,
    );

    inject_addressable(
        &mut kernel,
        &format!("{:0>64x}", 0xAB0001u64),
        &author_a,
        400,
        "art-a",
    );
    inject_addressable(
        &mut kernel,
        &format!("{:0>64x}", 0xAB0002u64),
        &author_b,
        600,
        "art-b",
    );

    let etag_shape = |target: &str| {
        let mut s = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        s.tags
            .insert("e".to_string(), BTreeSet::from([target.to_string()]));
        s
    };

    let mut shapes: Vec<(&str, InterestShape)> = Vec::new();

    // Single author, seeded → floored at newest.
    shapes.push((
        "single author+kind",
        InterestShape {
            authors: BTreeSet::from([author_a.clone()]),
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        },
    ));
    // Multi-author, both seeded → MIN across authors.
    shapes.push((
        "multi-author both seeded",
        InterestShape {
            authors: BTreeSet::from([author_a.clone(), author_b.clone()]),
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        },
    ));
    // Multi-author, one empty → abort (None).
    shapes.push((
        "multi-author one empty",
        InterestShape {
            authors: BTreeSet::from([author_a.clone(), make_pubkey(9_999)]),
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        },
    ));
    // #e thread reply, seeded → floored.
    shapes.push(("#e thread", etag_shape(&etag_target)));
    // Global kind-only (KindTime) → never floored.
    shapes.push((
        "global kind-only",
        InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        },
    ));
    // Uncovered no-kind → no floor.
    shapes.push(("no-kind", InterestShape::default()));
    // Addressable, both coords stored → MIN across coords.
    {
        let mut s = InterestShape {
            kinds: BTreeSet::from([30023u32]),
            ..Default::default()
        };
        s.addresses.insert(NaddrCoord {
            pubkey: author_a.clone(),
            kind: 30023,
            d_tag: "art-a".to_string(),
        });
        s.addresses.insert(NaddrCoord {
            pubkey: author_b.clone(),
            kind: 30023,
            d_tag: "art-b".to_string(),
        });
        shapes.push(("addressable both stored", s));
    }

    let truncated: HashSet<u64> = HashSet::new();
    let mut floored_seen = false;
    for (name, shape) in &shapes {
        let floor = super::floor::shape_floor(shape, kernel.store.as_ref(), &truncated);
        let unified = unified_floor(shape, kernel.store.as_ref());
        floored_seen |= floor.is_some();
        assert_eq!(
            floor, unified,
            "K3 Stage C lock violated for `{name}`: shape_floor ({floor:?}) and \
             watermark_from_queries ({unified:?}) disagree — a divergent second \
             shape→query mapping was reintroduced"
        );
    }
    assert!(
        floored_seen,
        "battery is vacuous — no seeded shape produced a floor"
    );
}
