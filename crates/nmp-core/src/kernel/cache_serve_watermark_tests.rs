//! ADR-0045 §6 / issue #1119 — structural watermark⇄serve seam-identity tests
//! (split from `cache_serve_budget_tests.rs` for the 500-LOC file ceiling;
//! shared fixtures live in `cache_serve_tests.rs` as `pub(super)` helpers).

use super::cache_serve::shape_to_store_queries;
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
