//! ADR-0045 §6 / issue #1119 — structural watermark⇄serve seam-identity tests
//! (split from `cache_serve_budget_tests.rs` for the 500-LOC file ceiling;
//! shared fixtures live in `cache_serve_tests.rs` as `pub(super)` helpers).

use super::cache_serve::{shape_to_store_queries, watermark_from_queries};
use super::cache_serve_tests::{hex_pk, seed_events};
use super::*;
use crate::planner::InterestShape;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::StoreQuery;
use std::collections::BTreeSet;

// ─── #1119: seam-identity guard (floored ⇒ served, by construction) ─────────

/// ADR-0045 §6 / issue #1119 — the floored⇒served guard is now STRUCTURAL,
/// not enumerative. `watermark_fn` consumes `shape_to_store_queries` as its
/// single source of shape semantics, so for ANY shape the implication
/// `watermark_for_shape(shape).is_some() ⇒ shape_to_store_queries(shape)` is
/// non-empty holds *by construction*: the watermark cannot produce a floor
/// without first deriving a non-empty query list to scan.
///
/// This test drives a broad, heterogeneous shape population (covered E1/E2/E3
/// shapes seeded with real stored events so the floor actually fires, plus
/// uncovered shapes that must refuse) through the REAL production
/// `watermark_fn` and asserts the implication for every one — replacing the
/// old hardcoded 4-shape case list with an exhaustive structural sweep.
#[test]
fn floored_implies_served_holds_structurally_for_any_shape() {
    use crate::planner::{InterestShape, NaddrCoord};

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    kernel.timeline_authors.insert(author.clone());
    seed_events(&mut kernel, &keys, 2, 1_700_000_000);

    // 64-char hex targets so hex decode succeeds and real queries are produced.
    let etag_target = hex_pk("e1");
    let ptag_target = hex_pk("fa");

    let mut shapes: Vec<(&str, InterestShape)> = Vec::new();

    // ── Covered shapes (serve is non-empty; floor may or may not fire) ───────
    shapes.push((
        "single-author+kind (seeded → floored)",
        InterestShape {
            authors: BTreeSet::from([author.clone()]),
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        },
    ));
    shapes.push((
        "multi-author one-empty (one author has no events → abort)",
        InterestShape {
            authors: BTreeSet::from([author.clone(), hex_pk("ee")]),
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        },
    ));
    shapes.push((
        "kindtime global feed (never floored)",
        InterestShape {
            kinds: BTreeSet::from([30023u32]),
            ..Default::default()
        },
    ));
    {
        let mut s = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        s.tags
            .insert("e".to_string(), BTreeSet::from([etag_target.clone()]));
        shapes.push(("#e thread reply", s));
    }
    {
        let mut s = InterestShape {
            kinds: BTreeSet::from([1059u32]),
            ..Default::default()
        };
        s.tags
            .insert("p".to_string(), BTreeSet::from([ptag_target.clone()]));
        shapes.push(("#p DM inbox", s));
    }
    {
        let mut s = InterestShape {
            kinds: BTreeSet::from([30023u32]),
            ..Default::default()
        };
        s.addresses.insert(NaddrCoord {
            pubkey: author.clone(),
            kind: 30023,
            d_tag: "my-article".to_string(),
        });
        shapes.push(("addressable long-form", s));
    }

    // ── Uncovered shapes (serve empty → must NOT be floored) ─────────────────
    shapes.push(("no-kinds", InterestShape::default()));
    {
        let mut s = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        s.event_ids.insert(hex_pk("1d"));
        shapes.push(("event-ids", s));
    }
    {
        // multi-tag → uncovered.
        let mut s = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        s.tags
            .insert("e".to_string(), BTreeSet::from([etag_target.clone()]));
        s.tags
            .insert("p".to_string(), BTreeSet::from([ptag_target.clone()]));
        shapes.push(("multi-tag", s));
    }
    {
        // multi-value single key → uncovered.
        let mut s = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        s.tags.insert(
            "e".to_string(),
            BTreeSet::from([etag_target.clone(), hex_pk("e2")]),
        );
        shapes.push(("multi-value tag", s));
    }
    {
        // 3-char (non-hex) #e target → hex decode fails → uncovered.
        let mut s = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        s.tags
            .insert("e".to_string(), BTreeSet::from(["abc".to_string()]));
        shapes.push(("non-hex #e target", s));
    }

    let mut floored_seen = false;
    for (name, shape) in &shapes {
        let floored = kernel
            .lifecycle
            .watermark_for_shape_for_test(shape)
            .is_some();
        let served = !shape_to_store_queries(shape).is_empty();
        floored_seen |= floored;
        assert!(
            !floored || served,
            "§6/#1119 violated for `{name}`: watermark floors but \
             shape_to_store_queries is empty — the two are no longer one table \
             read two ways"
        );
    }
    // Non-vacuity: at least one shape in the population IS floored, so the
    // implication is exercised in its non-trivial arm.
    assert!(
        floored_seen,
        "guard is vacuous — no seeded shape produced a watermark floor"
    );
}

/// #1119 follow-up 3 — pin the long-form variant explicitly. The structural
/// guard proves serve covers the shape; this asserts the *specific* StoreQuery
/// variant rather than letting a KindTime fallthrough satisfy it accidentally.
#[test]
fn longform_shape_maps_to_kind_dtag_only() {
    use crate::planner::{InterestShape, NaddrCoord};

    let mut longform_shape = InterestShape {
        kinds: BTreeSet::from([30023u32]),
        ..Default::default()
    };
    longform_shape.addresses.insert(NaddrCoord {
        pubkey: hex_pk("ab"),
        kind: 30023,
        d_tag: "the-d-tag".to_string(),
    });

    let queries = shape_to_store_queries(&longform_shape);
    assert!(
        matches!(queries.as_slice(), [StoreQuery::KindDtag { .. }]),
        "long-form shape must map to exactly one KindDtag query (not a \
         KindTime fallthrough); got {queries:?}"
    );
}

// ─── K3 Stage B1: address-pointer branch min/abort alignment ────────────────

/// Build a multi-coord addressable shape (two distinct `NaddrCoord`s, so
/// `shape_to_store_queries` yields two `KindDtag` queries).
fn multi_coord_addressable_shape() -> crate::planner::InterestShape {
    use crate::planner::{InterestShape, NaddrCoord};
    let mut shape = InterestShape {
        kinds: BTreeSet::from([30023u32]),
        ..Default::default()
    };
    shape.addresses.insert(NaddrCoord {
        pubkey: hex_pk("ab"),
        kind: 30023,
        d_tag: "coord-stored".to_string(),
    });
    shape.addresses.insert(NaddrCoord {
        pubkey: hex_pk("ab"),
        kind: 30023,
        d_tag: "coord-unfetched".to_string(),
    });
    shape
}

/// K3 Stage B1 ORACLE — a multi-coord addressable shape where ONE coordinate
/// has no stored event must NOT be floored.
///
/// The authors branch of the watermark fold takes the MIN across authors and
/// returns `None` (no floor) if ANY author has zero stored events, so a
/// newly-followed author is never floored above their unfetched history. The
/// address-pointer (`KindDtag`) branch took the opposite policy — MAX across
/// coords, ignoring coords with no stored match — so it would floor `since`
/// above an unfetched replaceable coordinate that then never arrives below the
/// floor. This test drives `watermark_from_queries` (the production fold) with
/// a `scan` that returns a timestamp for one coord and `None` for the other,
/// and asserts the abort: no floor, so the unfetched coord backfills in full.
///
/// FAILS on pre-B1 master (the `addr_max` branch ignores the empty coord and
/// returns the populated coord's `max`), passes after B1 aligns the branch with
/// the authors min/abort rule.
#[test]
fn addressable_shape_with_one_unfetched_coord_is_not_floored() {
    let shape = multi_coord_addressable_shape();
    let queries = shape_to_store_queries(&shape);
    assert_eq!(
        queries.len(),
        2,
        "multi-coord addressable shape must map to two KindDtag queries; got {queries:?}"
    );

    // `scan` returns a stored timestamp for the first coord and `None`
    // (unfetched) for the second — exactly the "partially-known multi-coord
    // shape" hazard B1 addresses. Match on the `d_tag` to decide.
    let floor = watermark_from_queries(
        &shape,
        |q| match q {
            StoreQuery::KindDtag { d_tag, .. } if d_tag == b"coord-stored" => Some(1_700_000_000),
            StoreQuery::KindDtag { d_tag, .. } if d_tag == b"coord-unfetched" => None,
            other => panic!("unexpected query in addressable fold: {other:?}"),
        },
        |_key| false,
    );
    assert_eq!(
        floor, None,
        "an addressable shape with an unfetched coordinate must NOT be floored — \
         the address-pointer branch must use the authors min/abort rule (any \
         coord with zero stored matches ⇒ no floor), not max-ignoring-empties"
    );
}

/// B1 companion — when EVERY coordinate has a stored event, the floor is the
/// MIN across coords (so no coord is floored above its own newest stored
/// event), matching the authors-branch semantics. Pre-B1 this returned the MAX.
#[test]
fn addressable_shape_with_all_coords_stored_floors_at_min() {
    let shape = multi_coord_addressable_shape();
    let floor = watermark_from_queries(
        &shape,
        |q| match q {
            StoreQuery::KindDtag { d_tag, .. } if d_tag == b"coord-stored" => Some(1_700_000_500),
            StoreQuery::KindDtag { d_tag, .. } if d_tag == b"coord-unfetched" => {
                Some(1_700_000_100)
            }
            other => panic!("unexpected query in addressable fold: {other:?}"),
        },
        |_key| false,
    );
    assert_eq!(
        floor,
        Some(1_700_000_100),
        "with all coords stored the floor must be the MIN across coords (so the \
         older coord is not floored above its newest stored event), matching the \
         authors-branch min rule"
    );
}

// ─── K3 Stage B3: Etag/Ptag truncated-serve floor refusal ───────────────────

/// Insert one kind:1 event that `#e`-tags `target_hex` into the store (the
/// thread-reply shape the `Etag` query indexes). Uses the same store-handle
/// insert path the K3 nip77 fixtures use, so the event is real index data.
fn insert_etag_event(kernel: &mut Kernel, id_hex: &str, target_hex: &str, created_at: u64) {
    use crate::store::{RawEvent, VerifiedEvent};
    let raw = RawEvent {
        id: id_hex.to_string(),
        pubkey: hex_pk("aa"),
        created_at,
        kind: 1,
        tags: vec![vec!["e".to_string(), target_hex.to_string()]],
        content: String::new(),
        sig: "a".repeat(128),
    };
    kernel
        .event_store_handle()
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://cache.example".to_string(),
            created_at.saturating_mul(1_000),
        )
        .expect("etag event insert");
}

/// Build the `#e` thread-reply interest shape for `target_hex`.
fn etag_thread_shape(target_hex: &str) -> InterestShape {
    let mut shape = InterestShape {
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    shape
        .tags
        .insert("e".to_string(), BTreeSet::from([target_hex.to_string()]));
    shape
}

/// K3 Stage B3 ORACLE — a budget-truncated `Etag` serve does NOT floor that
/// shape's REQ.
///
/// `Etag`/`Ptag` store-serve queries carry no resume cursor. When the per-tick
/// budget is exhausted mid-serve the chunk advances PAST the query, silently
/// skipping the stored tail *within serve depth*. Since ADR-0045 E2/E3 enabled
/// the watermark floor for these shapes, the relay would then never re-send the
/// skipped stored events that session — a permanent hole. The fix records the
/// budget-truncation and the watermark refuses to floor the shape, so the relay
/// re-sends the gap.
///
/// Orchestration (deterministic, no clock/relay): `visible_limit = 4`, so the
/// aggregate tick budget is `2 × 4 = 8`. Two author serves are capped at
/// `limit = 3` (so each serves depth 3 and consumes 3 of the budget), and the
/// `Etag` serve is reached with only `8 − 3 − 3 = 2` budget left:
///
/// - Serve A (author, `limit = 3`, 3 stored kind:1) → consumes 3 budget.
/// - Serve B (author, `limit = 3`, 3 stored kind:1) → consumes 3 budget.
///   Budget left: 2.
/// - Serve C (the `Etag` thread shape, 6 stored matches, depth 4) → its
///   `visit_limit = min(tick_remaining = 2, remaining_depth = 4) = 2`, so it
///   visits 2 (NOT exhausted, 6 exist) and `remaining_depth = 2 > 0` →
///   BUDGET-truncated within depth. The cursor-less branch records the
///   truncation.
///
/// The in-memory `events`/`timeline` caches are cleared after seeding so the
/// aggregate-window floor (which only engages once the timeline is full) and
/// live→serve dedup do not perturb the per-serve visit budget.
///
/// The watermark for the `Etag` shape must therefore be `None` (no floor).
///
/// FAILS on pre-B3 master (the floor is the newest stored Etag match's
/// timestamp), passes after B3's truncated-serve refusal lands.
#[test]
fn budget_truncated_etag_serve_is_not_floored() {
    use super::cache_serve::completion_key_for_interest;
    use super::cache_serve_tests::simulate_cold_restart;

    let mut kernel = Kernel::new(4); // visible_limit = 4 → budget 8
    let base_ts: u64 = 1_700_000_000;

    // ── Serve A: author capped at limit 3, 3 stored kind:1 (consumes 3) ──────
    let keys_a = ::nostr::Keys::generate();
    let author_a = keys_a.public_key().to_hex();
    kernel.timeline_authors.insert(author_a.clone());
    seed_events(&mut kernel, &keys_a, 3, base_ts);
    let shape_a = InterestShape {
        authors: BTreeSet::from([author_a.clone()]),
        kinds: BTreeSet::from([1u32]),
        limit: Some(3),
        ..Default::default()
    };

    // ── Serve B: a second author, same shape (consumes 3) ────────────────────
    let keys_b = ::nostr::Keys::generate();
    let author_b = keys_b.public_key().to_hex();
    kernel.timeline_authors.insert(author_b.clone());
    seed_events(&mut kernel, &keys_b, 3, base_ts + 100);
    let shape_b = InterestShape {
        authors: BTreeSet::from([author_b.clone()]),
        kinds: BTreeSet::from([1u32]),
        limit: Some(3),
        ..Default::default()
    };

    // ── Serve C: the Etag thread shape with 6 stored matches (depth 4) ───────
    let target_hex = hex_pk("e7");
    for i in 0..6u64 {
        insert_etag_event(
            &mut kernel,
            &hex_pk(&format!("c{i}")),
            &target_hex,
            base_ts + 200 + i,
        );
    }
    let shape_c = etag_thread_shape(&target_hex);

    // Sanity: the Etag shape maps to exactly one (cursor-less) Etag query.
    assert!(
        matches!(
            shape_to_store_queries(&shape_c).as_slice(),
            [StoreQuery::Etag { .. }]
        ),
        "Etag thread shape must map to one Etag query"
    );

    // Drop the in-memory caches + any serves the seeding ingest queued so the
    // queue order below is exactly A, B, C with no dedup/aggregate-floor skew.
    simulate_cold_restart(&mut kernel);

    // Enqueue in order A, B, C (FIFO), then drain ONE aggregate-budget tick.
    let key_a = completion_key_for_interest(&crate::subs::SubKey::new(1), &shape_a);
    let key_b = completion_key_for_interest(&crate::subs::SubKey::new(2), &shape_b);
    let key_c = completion_key_for_interest(&crate::subs::SubKey::new(3), &shape_c);
    kernel.enqueue_cache_serve(&shape_a, key_a);
    kernel.enqueue_cache_serve(&shape_b, key_b);
    kernel.enqueue_cache_serve(&shape_c, key_c);
    kernel.run_cache_serve_step();

    // The Etag serve was budget-truncated within depth, so the watermark MUST
    // refuse to floor it (else the relay never re-sends the stranded tail).
    let floor = kernel.lifecycle.watermark_for_shape_for_test(&shape_c);
    assert_eq!(
        floor, None,
        "a budget-truncated Etag serve must NOT floor its REQ — the stored tail \
         was stranded within serve depth and the relay must re-send it (got a \
         floor of {floor:?})"
    );
}

/// B3 companion — a NON-truncated (fully-served) Etag shape IS still floored,
/// so the refusal is scoped to the truncation hazard and does not blanket-
/// disable the floor for thread shapes. A single Etag serve whose match count
/// is within both the budget AND serve depth completes without truncation, and
/// the watermark floors it at the newest stored match.
#[test]
fn fully_served_etag_shape_is_still_floored() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let base_ts: u64 = 1_700_000_000;
    let target_hex = hex_pk("e8");
    // 2 stored matches — well within budget and depth, so no truncation.
    insert_etag_event(&mut kernel, &hex_pk("d0"), &target_hex, base_ts);
    insert_etag_event(&mut kernel, &hex_pk("d1"), &target_hex, base_ts + 1);
    let shape = etag_thread_shape(&target_hex);

    kernel.clear_served_interest_shapes();
    kernel.enqueue_cache_serve(&shape, 0xC0DE);
    kernel.run_cache_serve_step();

    let floor = kernel.lifecycle.watermark_for_shape_for_test(&shape);
    assert_eq!(
        floor,
        Some(base_ts + 1),
        "a fully-served Etag shape (no truncation) must still be floored at its \
         newest stored match — B3 refuses the floor only for budget-truncated \
         serves"
    );
}
