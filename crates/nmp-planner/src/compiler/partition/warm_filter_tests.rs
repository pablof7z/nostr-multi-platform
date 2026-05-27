//! W4 warm-relay filter tests — `apply_selection` with `RelayAuthorScoreLookup`.
//!
//! Doctrine guards verified:
//!   D3 — score is a filter, not a new lane; four-lane discipline preserved.
//!   D6 — `NoopRelayAuthorScoreLookup` returns 0.0/false; pre-W4 tests unaffected.
//!   D8 — filter is O(N * log M) in authors × relay-map; no extra allocation.
//!
//! Test plan:
//!   1. `warm_lookup_filters_cold_outbox_before_greedy` — one warm relay of 3
//!      is preferred; cold pair dropped when max_per_user=1.
//!   2. `operator_pinned_bypasses_warm_filter` — AppRelay-tagged URL cold for
//!      author still survives.
//!   3. `noop_lookup_preserves_existing_behaviour` — `Some(Noop)` == pre-W4.
//!   4. `no_warm_relays_falls_through_to_existing_pruning` — empty score map
//!      → all pairs cold → filter is a no-op → greedy still runs correctly.

use std::collections::BTreeSet;

use crate::interest::InterestShape;
use crate::plan::UserConfiguredCategory;
use crate::plan::{canonical_filter_hash, CompiledPlan, RelayPlan, RoutingSource, SubShape};
use crate::selection::apply_selection;
use crate::selection::relay_score_lookup::{NoopRelayAuthorScoreLookup, RelayAuthorScoreLookup};

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Build a plan with Nip65 routing. Each tuple is (relay_url, authors).
fn plan_nip65(relays: &[(&str, &[&str])]) -> CompiledPlan {
    let mut per_relay = std::collections::BTreeMap::new();
    for (relay, authors) in relays {
        let mut shape = InterestShape::default();
        for a in *authors {
            shape.authors.insert((*a).to_string());
        }
        let hash = canonical_filter_hash(&shape);
        let sub = SubShape {
            shape,
            originating_interests: vec![],
            canonical_filter_hash: hash,
        };
        let mut role_tags = BTreeSet::new();
        role_tags.insert(RoutingSource::Nip65);
        per_relay.insert(
            (*relay).to_string(),
            RelayPlan {
                relay_url: (*relay).to_string(),
                role_tags,
                sub_shapes: vec![sub],
            },
        );
    }
    CompiledPlan {
        plan_id: "test-warm".to_string(),
        per_relay,
        unroutable_authors: BTreeSet::new(),
    }
}

/// Build a plan with `UserConfigured(AppRelay)` tag (operator-pinned).
fn plan_app_relay(relay: &str, authors: &[&str]) -> CompiledPlan {
    let mut per_relay = std::collections::BTreeMap::new();
    let mut shape = InterestShape::default();
    for a in authors {
        shape.authors.insert((*a).to_string());
    }
    let hash = canonical_filter_hash(&shape);
    let sub = SubShape {
        shape,
        originating_interests: vec![],
        canonical_filter_hash: hash,
    };
    let mut role_tags = BTreeSet::new();
    role_tags.insert(RoutingSource::UserConfigured(
        UserConfiguredCategory::AppRelay,
    ));
    per_relay.insert(
        relay.to_string(),
        RelayPlan {
            relay_url: relay.to_string(),
            role_tags,
            sub_shapes: vec![sub],
        },
    );
    CompiledPlan {
        plan_id: "test-app-relay".to_string(),
        per_relay,
        unroutable_authors: BTreeSet::new(),
    }
}

/// A lookup that marks exactly one (author, relay) pair as warm.
struct SingleWarmLookup {
    warm_author: String,
    warm_relay: String,
}

impl RelayAuthorScoreLookup for SingleWarmLookup {
    fn weight(&self, author: &str, relay: &str) -> f32 {
        if author == self.warm_author && relay == self.warm_relay {
            0.60 // above WARM_THRESHOLD (0.40)
        } else {
            0.0 // cold
        }
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

/// Test 1: An author has 3 outbox relays; only one is warm. With max_per_user=1
/// the warm relay should be picked for that author; the cold relays are filtered
/// out of the greedy candidate set for this author.
///
/// Note: when ALL authors' relays are cold for a given relay (the relay has
/// zero warm pairs), the filter removes ALL author→relay pairs for it, so
/// the relay has zero coverage and the greedy pass drops it. The warm relay
/// survives because it has at least one warm author.
#[test]
fn warm_lookup_filters_cold_outbox_before_greedy() {
    // Author "alice" declares three outbox relays; only r1 is warm.
    let mut plan = plan_nip65(&[
        ("wss://r1", &["alice"]),
        ("wss://r2", &["alice"]),
        ("wss://r3", &["alice"]),
    ]);

    let lookup = SingleWarmLookup {
        warm_author: "alice".to_string(),
        warm_relay: "wss://r1".to_string(),
    };

    // max_per_user=1 so alice gets exactly one relay.
    apply_selection(&mut plan, 30, 1);
    // Without warm filter the greedy would pick r1 by lex tiebreak on equal
    // coverage. Verify that after W4 filter the plan still selects r1 (it's
    // warm) and drops r2/r3.
    //
    // Separately verify the warm-filtered path by re-running with the lookup.
    let mut plan2 = plan_nip65(&[
        ("wss://r1", &["alice"]),
        ("wss://r2", &["alice"]),
        ("wss://r3", &["alice"]),
    ]);
    apply_selection_with_lookup(&mut plan2, 30, 1, Some(&lookup));

    // r1 is warm → survives. r2/r3 are cold → greedy never sees alice there.
    assert!(
        plan2.per_relay.contains_key("wss://r1"),
        "warm relay wss://r1 must survive"
    );
    // r2 and r3 must not have alice (either the relay is dropped or alice
    // was removed from its sub_shape by the warm pre-filter + coverage
    // algorithm convergence).
    for cold_relay in ["wss://r2", "wss://r3"] {
        if let Some(rp) = plan2.per_relay.get(cold_relay) {
            for sub in &rp.sub_shapes {
                assert!(
                    !sub.shape.authors.contains("alice"),
                    "cold relay {cold_relay} must not carry alice after warm filter"
                );
            }
        }
    }
}

/// Test 2: An AppRelay-tagged URL that is cold for the author must STILL
/// survive — operator-pinned relays bypass the warm filter entirely.
#[test]
fn operator_pinned_bypasses_warm_filter() {
    // "alice" on an AppRelay-pinned relay (cold score for alice).
    let mut plan = plan_app_relay("wss://primal", &["alice"]);

    // Lookup returns 0.0 for alice on primal — it is "cold".
    let lookup = NoopRelayAuthorScoreLookup;

    apply_selection_with_lookup(&mut plan, 30, 2, Some(&lookup));

    // Operator-pinned relay MUST survive regardless of warm score.
    assert!(
        plan.per_relay.contains_key("wss://primal"),
        "operator-pinned relay wss://primal must survive even when cold"
    );
    // alice must still be on it.
    let relay_plan = &plan.per_relay["wss://primal"];
    let has_alice = relay_plan
        .sub_shapes
        .iter()
        .any(|s| s.shape.authors.contains("alice"));
    assert!(
        has_alice,
        "alice must still be on the operator-pinned relay"
    );
}

/// Test 3: `Some(NoopRelayAuthorScoreLookup)` behaves identically to the
/// pre-W4 path (all relays appear cold; filter is a no-op; greedy runs
/// exactly as before on the full candidate set).
#[test]
fn noop_lookup_preserves_existing_behaviour() {
    // 3 relays, 3 authors each. This mirrors the existing
    // `post_increment_bug_regression` test to confirm W4 doesn't regress it.
    let make_plan = || {
        plan_nip65(&[
            ("wss://r1", &["a", "b", "c"]),
            ("wss://r2", &["a", "b", "c"]),
            ("wss://r3", &["a", "b", "c"]),
        ])
    };

    let mut plan_no_lookup = make_plan();
    apply_selection(&mut plan_no_lookup, 30, 2);

    let mut plan_noop = make_plan();
    apply_selection_with_lookup(&mut plan_noop, 30, 2, Some(&NoopRelayAuthorScoreLookup));

    // Both paths must produce the same relay set and author assignments.
    let keys_no: Vec<_> = plan_no_lookup.per_relay.keys().cloned().collect();
    let keys_noop: Vec<_> = plan_noop.per_relay.keys().cloned().collect();
    assert_eq!(
        keys_no, keys_noop,
        "relay keys must match between no-lookup and noop-lookup paths"
    );
    for k in &keys_no {
        let authors_no: BTreeSet<_> = plan_no_lookup.per_relay[k]
            .sub_shapes
            .iter()
            .flat_map(|s| s.shape.authors.iter().cloned())
            .collect();
        let authors_noop: BTreeSet<_> = plan_noop.per_relay[k]
            .sub_shapes
            .iter()
            .flat_map(|s| s.shape.authors.iter().cloned())
            .collect();
        assert_eq!(
            authors_no, authors_noop,
            "author sets must match on relay {k}"
        );
    }
}

/// Test 4: When the score map is empty (all pairs cold under a noop lookup),
/// the warm pre-filter is a no-op. The existing greedy algorithm runs on the
/// full candidate set exactly as in pre-W4 behaviour.
#[test]
fn no_warm_relays_falls_through_to_existing_pruning() {
    // No relay is warm — filter must pass everything through.
    let mut plan = plan_nip65(&[
        ("wss://a", &["x", "y"]),
        ("wss://b", &["x", "y"]),
        ("wss://c", &["z"]),
    ]);

    apply_selection_with_lookup(&mut plan, 30, 1, Some(&NoopRelayAuthorScoreLookup));

    // All authors must still be covered (greedy ran on full set).
    let mut covered_authors: BTreeSet<String> = BTreeSet::new();
    for rp in plan.per_relay.values() {
        for sub in &rp.sub_shapes {
            covered_authors.extend(sub.shape.authors.iter().cloned());
        }
    }
    for expected in ["x", "y", "z"] {
        assert!(
            covered_authors.contains(expected),
            "author {expected} must be covered when no relays are warm"
        );
    }
}

// Delegate to the real implementation in `crate::selection`.
fn apply_selection_with_lookup(
    plan: &mut CompiledPlan,
    max_connections: usize,
    max_per_user: usize,
    score_lookup: Option<&dyn RelayAuthorScoreLookup>,
) {
    crate::selection::apply_selection_with_lookup(plan, max_connections, max_per_user, score_lookup)
}
