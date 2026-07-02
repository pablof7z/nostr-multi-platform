//! Gap 4 + Gap 5: how per-relay bookkeeping accumulates and dedupes across
//! interests — `originating_interests` (per sub-shape) and `role_tags`
//! (per relay).

use super::{author_interest, pk, write_snapshot};
use crate::compiler::mailbox::{InMemoryMailboxCache, MailboxSnapshot};
use crate::compiler::SubscriptionCompiler;
use crate::interest::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::plan::RoutingSource;
use std::collections::BTreeSet;

// ── Gap 4: originating_interests dedup ──────────────────────────────────

/// An interest with explicit `authors` AND `#p` tag values fires both the
/// Case A outbox push and the "both populated" inbox push. When the
/// author's write relay and the tagged pubkey's read relay are the SAME
/// URL, the one interest_id lands on that relay twice — Stage 3 must
/// record it only once (`originating_interests` is a set, not a multiset).
#[test]
fn same_interest_on_one_relay_via_two_lanes_dedupes_originating_id() {
    let mut cache = InMemoryMailboxCache::new();
    // Alice (the author) writes to wss://shared.
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    // Carol (the #p-tagged recipient) READS from the very same wss://shared.
    cache.put(
        pk("carol"),
        MailboxSnapshot {
            write_relays: vec![],
            read_relays: vec!["wss://shared".to_string()],
            both_relays: vec![],
        },
    );
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    // One interest: author Alice + #p:[Carol].
    let mut tags = std::collections::BTreeMap::new();
    tags.insert(
        "p".to_string(),
        [pk("carol")].into_iter().collect::<BTreeSet<_>>(),
    );
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk("alice")].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };

    let plan = compiler.compile(&[interest]).expect("compile");
    let relay = plan.per_relay.get("wss://shared").expect("shared relay");

    // Across ALL sub-shapes on the relay, interest #1 must appear exactly
    // once per sub-shape's originating list — never duplicated.
    for sub in &relay.sub_shapes {
        let count = sub
            .originating_interests
            .iter()
            .filter(|id| **id == InterestId(1))
            .count();
        assert!(
            count <= 1,
            "interest id must be deduped within a sub-shape; saw it {count} times"
        );
    }
}

// ── Gap 5: role_tags accumulation across distinct interests ─────────────

/// One relay reached by two different interests via two different lanes
/// (author A via NIP-65, author B via AppRelay because the operator
/// pinned the same URL) must carry BOTH lanes in `role_tags` — the
/// four-lane discipline is preserved across interest boundaries, not just
/// within one interest.
#[test]
fn role_tags_accumulate_across_interests_on_a_shared_relay() {
    let mut cache = InMemoryMailboxCache::new();
    // Alice declares wss://shared as her NIP-65 write relay.
    cache.put(pk("alice"), write_snapshot(&["wss://shared"]));
    // Bob has no mailbox; he will only ride the app-relay lane.
    let app = vec!["wss://shared".to_string()];
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &app);

    let plan = compiler
        .compile(&[
            author_interest(1, &["alice"], &[1], InterestLifecycle::Tailing),
            author_interest(2, &["bob"], &[1], InterestLifecycle::Tailing),
        ])
        .expect("compile");

    let relay = plan.per_relay.get("wss://shared").expect("shared relay");
    assert!(
        relay.role_tags.contains(&RoutingSource::Nip65),
        "Alice's NIP-65 lane must be recorded"
    );
    assert!(
        relay.role_tags.contains(&RoutingSource::UserConfigured(
            crate::plan::UserConfiguredCategory::AppRelay
        )),
        "Bob's AppRelay lane must be recorded on the same relay"
    );
}
