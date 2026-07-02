//! Gap 2 + mixed-shape + address-pointer: what a single interest's
//! sub-shape looks like on the wire — filter contents, provenance, and the
//! canonical filter hash.

use super::{author_interest, pk, write_snapshot};
use crate::compiler::mailbox::InMemoryMailboxCache;
use crate::compiler::SubscriptionCompiler;
use crate::interest::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, NaddrCoord,
};
use crate::plan::canonical_filter_hash;
use std::collections::BTreeSet;

// ── Gap 2: single author interest → correct filter shape ────────────────

/// One author with a known NIP-65 write relay produces exactly one
/// `RelayPlan` carrying exactly one `SubShape`, whose shape echoes the
/// interest's authors+kinds and names the originating interest.
#[test]
fn single_author_interest_produces_one_subshape() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://alice-write"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[author_interest(
            1,
            &["alice"],
            &[1],
            InterestLifecycle::Tailing,
        )])
        .expect("compile");

    assert_eq!(plan.per_relay.len(), 1, "exactly one relay in the plan");
    let relay = plan
        .per_relay
        .get("wss://alice-write")
        .expect("alice-write relay");
    assert_eq!(relay.sub_shapes.len(), 1, "one interest → one sub-shape");

    let sub = &relay.sub_shapes[0];
    // Author-partitioning: the sub-shape's author set is exactly Alice.
    assert_eq!(
        sub.shape.authors,
        [pk("alice")].into_iter().collect::<BTreeSet<_>>()
    );
    assert_eq!(sub.shape.kinds, [1u32].into_iter().collect::<BTreeSet<_>>());
    // Provenance: the sub-shape names interest #1.
    assert_eq!(sub.originating_interests, vec![InterestId(1)]);
    // The cached hash matches a fresh hash of the shape.
    assert_eq!(sub.canonical_filter_hash, canonical_filter_hash(&sub.shape));
}

// ── Mixed-shape interests on one relay (timeline + profile) ─────────────

/// A timeline interest (kinds {1,6}, no limit) and a profile interest
/// (kinds {0,3,10002}, limit Some(3)) for the SAME author route to the
/// same write relay but cannot merge — different kinds (Rule 1) and a
/// limit on one side (Rule 5). The relay therefore carries two distinct
/// sub-shapes, each with the correct filter shape.
#[test]
fn timeline_and_profile_for_same_author_produce_two_subshapes() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://alice-write"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let timeline = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape::timeline_for(
            [pk("alice")].into_iter().collect(),
            [30023u32].into_iter().collect(),
        ),
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    let profile = LogicalInterest {
        id: InterestId(2),
        scope: InterestScope::Global,
        shape: InterestShape::profile_for(pk("alice")),
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    };

    let plan = compiler.compile(&[timeline, profile]).expect("compile");
    let relay = plan
        .per_relay
        .get("wss://alice-write")
        .expect("alice-write relay");
    assert_eq!(
        relay.sub_shapes.len(),
        2,
        "timeline and profile shapes cannot merge — two REQs on the relay"
    );

    // Exactly one sub-shape carries the timeline kinds, one the profile kinds.
    // V-68: `timeline_for` no longer injects {1, 6}; this test declares an
    // arbitrary host kind set ({30023}) to prove the constructor carries
    // caller policy verbatim and the compiler routes it without rewriting.
    let timeline_kinds: BTreeSet<u32> = [30023].into_iter().collect();
    let profile_kinds: BTreeSet<u32> = [0, 3, 10002].into_iter().collect();
    let has_timeline = relay
        .sub_shapes
        .iter()
        .any(|s| s.shape.kinds == timeline_kinds);
    let has_profile = relay
        .sub_shapes
        .iter()
        .any(|s| s.shape.kinds == profile_kinds);
    assert!(
        has_timeline,
        "one sub-shape must carry the host-declared timeline kinds {{30023}}"
    );
    assert!(
        has_profile,
        "one sub-shape must carry the profile kinds {{0,3,10002}}"
    );

    // The profile sub-shape preserves its limit (Rule 5 would have refused
    // any merge that dropped it).
    let profile_sub = relay
        .sub_shapes
        .iter()
        .find(|s| s.shape.kinds == profile_kinds)
        .expect("profile sub-shape");
    assert_eq!(
        profile_sub.shape.limit,
        Some(3),
        "profile limit must survive"
    );
}

/// A naddr-coordinate address pointer (Case B) routes to the addressed
/// author's write relay and produces a sub-shape whose `addresses` field
/// carries the coordinate verbatim.
#[test]
fn address_pointer_interest_routes_coord_to_authors_write_relay() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("author"), write_snapshot(&["wss://author-write"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let coord = NaddrCoord {
        pubkey: pk("author"),
        kind: 30023,
        d_tag: "long-form".to_string(),
    };
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [30023u32].into_iter().collect(),
            addresses: [coord.clone()].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    };

    let plan = compiler.compile(&[interest]).expect("compile");
    let relay = plan
        .per_relay
        .get("wss://author-write")
        .expect("author-write relay");
    assert_eq!(relay.sub_shapes.len(), 1, "one address pointer → one REQ");
    assert!(
        relay.sub_shapes[0].shape.addresses.contains(&coord),
        "the sub-shape must carry the naddr coordinate verbatim"
    );
}
