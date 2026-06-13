//! Defect 3 — Stage-3 greedy first-fit merge must be input-order-independent.
//!
//! The per-relay merge loop in `compile_with_context` visits shaped entries
//! in some order and greedily folds each into the first compatible group. The
//! merge lattice's value-union caps (Rule 7 `event_ids`, Rule 2 `tags`, Rule 8
//! `addresses`) make compatibility *non-transitive*: A may merge with B and B
//! with C, yet A+B+C overflow the cap. With no canonical pre-sort, the grouping
//! — and therefore the REQ count and filter sets — would depend on arrival
//! order, producing nondeterministic relay load. The compiler sorts entries by
//! a canonical key first; these tests pin that the output is a pure function of
//! the entry SET, not its order.

use super::*;
use crate::compiler::mailbox::InMemoryMailboxCache;
use crate::interest::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use std::collections::BTreeSet;

/// Deterministic 64-char hex pubkey fixture from a short label.
fn pk(label: &str) -> String {
    format!("{label:0>64}").chars().take(64).collect()
}

/// A NIP-65 snapshot whose write relays are the given URLs.
fn write_snapshot(write: &[&str]) -> MailboxSnapshot {
    MailboxSnapshot {
        write_relays: write.iter().map(|s| s.to_string()).collect(),
        read_relays: vec![],
        both_relays: vec![],
    }
}

/// A OneShot author interest carrying a block of distinct `event_ids`, all
/// routing to the same write relay. The `event_ids` union cap (Rule 7,
/// `DEFAULT_VALUE_LIMIT = 1000`) makes the greedy first-fit merge
/// non-transitive: any two of the three blocks fit (≤ 800), but all three
/// together overflow (1200), so the grouping the greedy loop produces
/// depends on the order it visits the entries.
fn event_id_block_interest(id: u64, base: u32) -> LogicalInterest {
    let ids: BTreeSet<String> = (base..base + 400).map(|i| format!("{i:064x}")).collect();
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk("alice")].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            event_ids: ids,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

/// Compile the three event-id blocks and return a canonical, order-stable
/// fingerprint of the merge result for `wss://shared`: the sorted set of
/// each sub-shape's sorted `event_ids` set. Two runs that produce the same
/// fingerprint are observationally identical wire output.
fn shared_relay_fingerprint(
    cache: &InMemoryMailboxCache,
    interests: &[LogicalInterest],
) -> Vec<Vec<String>> {
    let compiler = SubscriptionCompiler::new(cache, &[]);
    let plan = compiler.compile(interests).expect("compile");
    let relay = plan.per_relay.get("wss://shared").expect("shared relay");
    let mut groups: Vec<Vec<String>> = relay
        .sub_shapes
        .iter()
        .map(|s| {
            let mut ids: Vec<String> = s.shape.event_ids.iter().cloned().collect();
            ids.sort();
            ids
        })
        .collect();
    groups.sort();
    groups
}

/// The same three interests fed in two different orderings MUST yield an
/// identical merge result (same number of REQs, same filter sets). Without
/// a canonical pre-sort, the greedy first-fit loop is input-order-dependent
/// — different orderings group the blocks differently, yielding
/// nondeterministic relay load / possible filter explosion.
#[test]
fn stage3_merge_is_input_order_independent() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));

    let a = event_id_block_interest(1, 0);
    let b = event_id_block_interest(2, 400);
    let c = event_id_block_interest(3, 800);

    let order_abc = shared_relay_fingerprint(&cache, &[a.clone(), b.clone(), c.clone()]);
    let order_cba = shared_relay_fingerprint(&cache, &[c.clone(), b.clone(), a.clone()]);
    let order_bca = shared_relay_fingerprint(&cache, &[b, c, a]);

    assert_eq!(
        order_abc, order_cba,
        "merge output must be identical regardless of input order (abc vs cba)"
    );
    assert_eq!(
        order_abc, order_bca,
        "merge output must be identical regardless of input order (abc vs bca)"
    );
}
