//! ADR-0045 §6 / issue #1119 — structural watermark⇄serve seam-identity tests
//! (split from `cache_serve_budget_tests.rs` for the 500-LOC file ceiling;
//! shared fixtures live in `cache_serve_tests.rs` as `pub(super)` helpers).

use super::cache_serve::{shape_to_store_queries, watermark_from_queries};
use super::cache_serve_tests::{hex_pk, seed_events};
use super::*;
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
