//! NIP-29 integration tests.
//!
//! **Lattice Rule 9 relay-pin / h-tag coalesce** — two host-pinned
//! interests targeting different hosts refuse to merge; identical hosts
//! merge cleanly (Rule 2 unions h-tag values); the pin short-circuits
//! the four-lane partition (Case E).
//!
//! The former group-lifecycle and audit-only-moderation tests were deleted
//! alongside the `domain` / `view` modules they exercised.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::planner::{
    merge as lattice_merge, EmptyMailboxCache, InterestId, InterestLifecycle, InterestScope,
    InterestShape, LogicalInterest, MergeOutcome, SubscriptionCompiler,
};

use crate::group_id::GroupId;
use crate::interest::host_pinned_interest;

// ─── Lattice Rule 9 relay-pin / h-tag coalesce ──────────────────────────────

#[test]
fn nip29_lattice_rule9_relay_pin_blocks_cross_host_merge() {
    let g_a = GroupId::new("wss://relay-a.example.com", "room");
    let g_b = GroupId::new("wss://relay-b.example.com", "room");

    let i_a = host_pinned_interest(1, &g_a, [9], BTreeMap::new(), InterestLifecycle::Tailing);
    let i_b = host_pinned_interest(2, &g_b, [9], BTreeMap::new(), InterestLifecycle::Tailing);

    // Direct lattice check: refuse across hosts.
    let outcome = lattice_merge(&i_a.shape, &i_b.shape, &i_a.lifecycle, &i_b.lifecycle);
    assert_eq!(outcome, MergeOutcome::Refused, "different relay_pin must refuse merge");

    // End-to-end compiler check: pinned interests on different hosts each
    // produce their own per-relay plan (Case E short-circuits the four-lane
    // partition; the planner emits one frame per host).
    let cache = EmptyMailboxCache;
    let indexer: Vec<String> = vec!["wss://indexer.example.com".into()];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);
    let plan = compiler.compile(&[i_a.clone(), i_b.clone()]).expect("compile");
    assert!(plan.per_relay.contains_key(&g_a.host_relay_url));
    assert!(plan.per_relay.contains_key(&g_b.host_relay_url));
    // Indexer must NOT be reached — pinned interests skip the indexer fallback.
    assert!(!plan.per_relay.contains_key("wss://indexer.example.com"));

    // Identical pinned interests collapse to a single per-relay sub_shape with
    // the merged h-tag dimension (Rule 9 passes, Rule 2 unions h values).
    let mut i_c = host_pinned_interest(3, &g_a, [9], BTreeMap::new(), InterestLifecycle::Tailing);
    // Distinct interest_id so the compiler tracks both as originators.
    i_c.id = InterestId(3);
    let plan2 = compiler.compile(&[i_a.clone(), i_c.clone()]).expect("compile");
    let host_a_plan = plan2.per_relay.get(&g_a.host_relay_url).expect("host a present");
    // Same h tag value → one sub_shape (merged), not two.
    assert_eq!(host_a_plan.sub_shapes.len(), 1);
    let merged_h = host_a_plan.sub_shapes[0].shape.tags.get("h").unwrap();
    assert!(merged_h.contains("room"));

    // Sanity-check that an unpinned interest does not collapse into a pinned
    // one (Rule 9: None does NOT absorb Some).
    let unpinned = LogicalInterest {
        id: InterestId(99),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [9u32].into_iter().collect(),
            tags: {
                let mut m: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
                m.insert("h".into(), ["room".into()].into_iter().collect());
                m
            },
            relay_pin: None,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    };
    let outcome2 = lattice_merge(&i_a.shape, &unpinned.shape, &i_a.lifecycle, &unpinned.lifecycle);
    assert_eq!(outcome2, MergeOutcome::Refused, "None must not absorb Some(host)");
}
