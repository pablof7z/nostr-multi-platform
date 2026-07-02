//! Gap 3: two interests routing to the same relay — merge when the merge
//! lattice allows it, stay distinct sub-shapes when it refuses.

use super::{author_interest, pk, write_snapshot};
use crate::compiler::mailbox::InMemoryMailboxCache;
use crate::compiler::SubscriptionCompiler;
use crate::interest::{InterestId, InterestLifecycle};
use std::collections::BTreeSet;

// ── Gap 3: two compatible interests for the same relay → merged ─────────

/// Two interests with mergeable shapes (same kinds, same lifecycle) that
/// route to the SAME relay collapse into a single `SubShape`. Stage 3's
/// greedy merge unions the author sets and records BOTH originating
/// interest ids on the one sub-shape.
#[test]
fn two_compatible_interests_same_relay_merge_into_one_subshape() {
    let mut cache = InMemoryMailboxCache::new();
    // Two distinct authors, both publishing to the same write relay.
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    cache.put(pk("bob"), write_snapshot(&["wss://shared"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[
            author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
            author_interest(2, &["bob"], &[1], InterestLifecycle::Tailing),
        ])
        .expect("compile");

    let relay = plan.per_relay.get("wss://shared").expect("shared relay");
    assert_eq!(
        relay.sub_shapes.len(),
        1,
        "two mergeable interests on one relay collapse into one REQ"
    );
    let sub = &relay.sub_shapes[0];
    // Merged shape unions both authors.
    assert_eq!(
        sub.shape.authors,
        [pk("alice"), pk("bob")]
            .into_iter()
            .collect::<BTreeSet<_>>()
    );
    // Both interest ids are recorded on the merged sub-shape.
    let ids: BTreeSet<InterestId> = sub.originating_interests.iter().cloned().collect();
    assert_eq!(ids, [InterestId(1), InterestId(2)].into_iter().collect());
}

// ── Gap 3 (refusal): two incompatible interests → two sub-shapes ────────

/// Two interests that route to the same relay but FAIL the merge lattice
/// (here Rule 1 — different kind sets) produce TWO distinct `SubShape`s
/// on the one `RelayPlan`: one wire REQ each.
#[test]
fn incompatible_kinds_same_relay_stay_distinct_subshapes() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    cache.put(pk("bob"), write_snapshot(&["wss://shared"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[
            // kind:1 — text notes.
            author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
            // kind:30023 — long-form. Rule 1 refuses (distinct, no wildcard).
            author_interest(2, &["bob"], &[30023], InterestLifecycle::Tailing),
        ])
        .expect("compile");

    let relay = plan.per_relay.get("wss://shared").expect("shared relay");
    assert_eq!(
        relay.sub_shapes.len(),
        2,
        "incompatible kind sets must NOT merge — two REQs on the relay"
    );
}

/// Two interests on the same relay with different LIFECYCLES (Tailing vs
/// OneShot) fail Rule 6 and stay as two `SubShape`s — the wire-emitter
/// needs distinct frames so it can CLOSE the one-shot REQ on EOSE while
/// leaving the tailing one open.
#[test]
fn mixed_lifecycle_same_relay_stays_distinct_subshapes() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    cache.put(pk("bob"), write_snapshot(&["wss://shared"]));
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[
            author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
            author_interest(2, &["bob"], &[1], InterestLifecycle::OneShot),
        ])
        .expect("compile");

    let relay = plan.per_relay.get("wss://shared").expect("shared relay");
    assert_eq!(
        relay.sub_shapes.len(),
        2,
        "Rule 6 refuses cross-lifecycle merges — two REQs on the relay"
    );
}
