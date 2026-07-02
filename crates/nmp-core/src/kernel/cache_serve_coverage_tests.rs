//! Issue #1517 — full StoreQuery coverage audit for `shape_to_store_queries`.
//!
//! Split from `cache_serve_budget_tests.rs` to respect the 500-LOC ceiling.
//! Shared fixtures live in `cache_serve_tests` as `pub(super)` helpers.

use super::cache_serve::shape_to_store_queries;
use crate::store::StoreQuery;
use std::collections::BTreeSet;

// ─── Issue #1517: full Scope shape coverage audit ────────────────────────────

/// ADR-0070 §6 acceptance (issue #1517): every named cache-serve Scope shape
/// resolves to a bounded StoreQuery on a named index, OR is in the explicit
/// intentionally-uncovered set. Table-driven over the full Scope shape list —
/// the "one-seam/every-interest contract" guard.
///
/// See also: `e1_watermark_serve_invariant_shapes_are_aligned`,
/// `e3_structural_floored_implies_served` (§6 watermark-invariant guards).
#[test]
fn issue_1517_every_scope_shape_has_a_plan_or_tracked_exception() {
    use crate::planner::{InterestShape, NaddrCoord};

    let a1 = "0100000000000000000000000000000000000000000000000000000000000000";
    let a2 = "0200000000000000000000000000000000000000000000000000000000000000";
    let pk_hex = "fa00000000000000000000000000000000000000000000000000000000000000";
    let event_id_hex = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1";
    let id1 = event_id_hex;

    // Covered shapes: expect non-empty StoreQuery list.
    let covered: &[(&str, InterestShape)] = &[
        (
            "author-kind timeline",
            InterestShape {
                authors: BTreeSet::from([a1.to_string()]),
                kinds: BTreeSet::from([1u32]),
                ..Default::default()
            },
        ),
        (
            "follow feed multi-author",
            InterestShape {
                authors: BTreeSet::from([a1.to_string(), a2.to_string()]),
                kinds: BTreeSet::from([1u32]),
                ..Default::default()
            },
        ),
        (
            "global feed kind-time",
            InterestShape {
                kinds: BTreeSet::from([1u32]),
                ..Default::default()
            },
        ),
        (
            "relay-kind diagnostics",
            InterestShape {
                kinds: BTreeSet::from([10002u32]),
                ..Default::default()
            },
        ),
        (
            "thread replies etag",
            {
                let mut s = InterestShape {
                    kinds: BTreeSet::from([1u32]),
                    ..Default::default()
                };
                s.tags
                    .insert("e".to_string(), BTreeSet::from([event_id_hex.to_string()]));
                s
            },
        ),
        (
            "dm ciphertext replay ptag",
            {
                let mut s = InterestShape {
                    kinds: BTreeSet::from([1059u32]),
                    ..Default::default()
                };
                s.tags
                    .insert("p".to_string(), BTreeSet::from([pk_hex.to_string()]));
                s
            },
        ),
        (
            "mention inbox ptag",
            {
                let mut s = InterestShape {
                    kinds: BTreeSet::from([9735u32]),
                    ..Default::default()
                };
                s.tags
                    .insert("p".to_string(), BTreeSet::from([pk_hex.to_string()]));
                s
            },
        ),
        (
            "long-form addressable kinddtag",
            {
                let mut s = InterestShape {
                    kinds: BTreeSet::from([30023u32]),
                    ..Default::default()
                };
                s.addresses.insert(NaddrCoord {
                    pubkey: pk_hex.to_string(),
                    kind: 30023,
                    d_tag: "slug".to_string(),
                });
                s
            },
        ),
        (
            // #t hashtag feed — now locally hydratable via the generic tag path
            // (previously a tracked "unrecognized tag key" exception).
            "hashtag feed tags",
            {
                let mut s = InterestShape {
                    kinds: BTreeSet::from([1u32]),
                    ..Default::default()
                };
                s.tags
                    .insert("t".to_string(), BTreeSet::from(["nostr".to_string()]));
                s
            },
        ),
        (
            // Multi-tag AND (#e ∩ #p) — now ONE exact `Tags` query (previously a
            // tracked "multi-tag intersection" exception).
            "multi-tag intersection tags",
            {
                let mut s = InterestShape {
                    kinds: BTreeSet::from([1u32]),
                    ..Default::default()
                };
                s.tags
                    .insert("e".to_string(), BTreeSet::from([id1.to_string()]));
                s.tags
                    .insert("p".to_string(), BTreeSet::from([pk_hex.to_string()]));
                s
            },
        ),
        (
            "profile metadata author",
            InterestShape {
                authors: BTreeSet::from([a1.to_string()]),
                kinds: BTreeSet::from([0u32]),
                ..Default::default()
            },
        ),
    ];

    for (label, shape) in covered {
        let queries = shape_to_store_queries(shape);
        assert!(
            !queries.is_empty(),
            "covered shape `{label}` must produce a non-empty StoreQuery list"
        );
    }

    // Uncovered shapes (tracked exceptions): expect empty StoreQuery list.
    let uncovered: &[(&str, InterestShape)] = &[
        ("wildcard kinds uncovered", InterestShape::default()),
        (
            "event-ids only uncovered",
            {
                let mut s = InterestShape {
                    kinds: BTreeSet::from([1u32]),
                    ..Default::default()
                };
                s.event_ids.insert(id1.to_string());
                s
            },
        ),
        (
            // addresses + generic tags together cannot be one exact query.
            "addresses with tags uncovered",
            {
                let mut s = InterestShape {
                    kinds: BTreeSet::from([30023u32]),
                    ..Default::default()
                };
                s.addresses.insert(NaddrCoord {
                    pubkey: pk_hex.to_string(),
                    kind: 30023,
                    d_tag: "slug".to_string(),
                });
                s.tags
                    .insert("t".to_string(), BTreeSet::from(["nostr".to_string()]));
                s
            },
        ),
    ];

    for (label, shape) in uncovered {
        let queries = shape_to_store_queries(shape);
        assert!(
            queries.is_empty(),
            "uncovered (tracked exception) shape `{label}` must produce an empty \
             StoreQuery list — if this now maps to a query, promote it to the \
             covered set and update the doc comment in queries.rs"
        );
    }
}

/// Pins the exact StoreQuery variant each covered Scope shape produces,
/// so a StoreQuery rename or accidental fallback fails loudly.
/// Each StoreQuery variant maps to a named LMDB index
/// (idx_author_kind, idx_kind_time, idx_kind_dtag_time, and the
/// `tci`/`atci`/`ktci` generic-tag indexes for `Tags`) per nmp_store::StoreQuery.
///
/// See `issue_1517_every_scope_shape_has_a_plan_or_tracked_exception`
/// for the full Scope coverage guard (this test pins the variant only
/// for covered shapes).
#[test]
fn issue_1517_covered_shapes_map_to_expected_index_variant() {
    use crate::planner::{InterestShape, NaddrCoord};

    let a1 = "0100000000000000000000000000000000000000000000000000000000000000";
    let a2 = "0200000000000000000000000000000000000000000000000000000000000000";
    let pk_hex = "fa00000000000000000000000000000000000000000000000000000000000000";
    let event_id_hex = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1";

    let single_author = InterestShape {
        authors: BTreeSet::from([a1.to_string()]),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    let multi_author = InterestShape {
        authors: BTreeSet::from([a1.to_string(), a2.to_string()]),
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    let global_feed = InterestShape {
        kinds: BTreeSet::from([1u32]),
        ..Default::default()
    };
    let thread_etag = {
        let mut s = InterestShape {
            kinds: BTreeSet::from([1u32]),
            ..Default::default()
        };
        s.tags
            .insert("e".to_string(), BTreeSet::from([event_id_hex.to_string()]));
        s
    };
    let dm_ptag = {
        let mut s = InterestShape {
            kinds: BTreeSet::from([1059u32]),
            ..Default::default()
        };
        s.tags
            .insert("p".to_string(), BTreeSet::from([pk_hex.to_string()]));
        s
    };
    let addressable = {
        let mut s = InterestShape {
            kinds: BTreeSet::from([30023u32]),
            ..Default::default()
        };
        s.addresses.insert(NaddrCoord {
            pubkey: pk_hex.to_string(),
            kind: 30023,
            d_tag: "slug".to_string(),
        });
        s
    };

    let checks: &[(&str, &InterestShape, &str)] = &[
        ("single-author", &single_author, "AuthorKind"),
        ("multi-author", &multi_author, "AuthorsKind"),
        ("global-feed", &global_feed, "KindTime"),
        ("thread-etag", &thread_etag, "Tags"),
        ("dm-ptag", &dm_ptag, "Tags"),
        ("addressable", &addressable, "KindDtag"),
    ];

    for (label, shape, expected) in checks {
        let queries = shape_to_store_queries(shape);
        assert!(!queries.is_empty(), "{label}: expected non-empty StoreQuery list");
        let variant_ok = match &queries[0] {
            StoreQuery::AuthorKind { .. } => *expected == "AuthorKind",
            StoreQuery::AuthorsKind { .. } => *expected == "AuthorsKind",
            StoreQuery::KindTime { .. } => *expected == "KindTime",
            StoreQuery::KindDtag { .. } => *expected == "KindDtag",
            StoreQuery::Tags { .. } => *expected == "Tags",
        };
        assert!(
            variant_ok,
            "{label}: expected {expected} variant, got a different StoreQuery variant — \
             update this test and the LMDB index column in queries.rs doc comment"
        );
    }
}
