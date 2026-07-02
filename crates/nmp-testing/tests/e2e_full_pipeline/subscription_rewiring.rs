//! Test 2 — kind3_update_rewires_subscriptions
//!
//! Scenario:
//!   1. Build a SubscriptionLifecycle with alice registered (tailing interest).
//!   2. Compile: assert REQ targets wss://alice-relay/.
//!   3. Enqueue a FollowListChanged trigger adding carol.
//!   4. Wire carol's mailbox and expand the interest.
//!   5. drain_tick: assert the returned WireFrames include a REQ for carol's relay.
//!   6. Idempotence: second drain with empty inbox emits no frames.
//!
//! "ContactListView snapshot reflects [alice, carol]" is implemented at the
//! routing layer (WireFrame) — that is the real observable for subscription
//! rewiring.  The actor's update channel is opaque to outbound REQs.

use crate::support::{padded_pubkey, put_write_mailbox, req_relays};

#[test]
fn kind3_update_rewires_subscriptions() {
    use nmp_core::subs::{AccountId, CompileTrigger, SubscriptionLifecycle};
    use nmp_planner::{
        InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
        LogicalInterest,
    };
    use std::collections::BTreeSet;

    fn tailing_interest(id: u64, authors: &[&str]) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::ActiveAccount,
            shape: InterestShape {
                authors: authors
                    .iter()
                    .map(|a| padded_pubkey(a))
                    .collect::<BTreeSet<_>>(),
                kinds: [1u32].into_iter().collect(),
                ..Default::default()
            },
            hints: vec![],
            lifecycle: InterestLifecycle::Tailing,
            is_indexer_discovery: false,
        }
    }

    let mut lc = SubscriptionLifecycle::new();
    let mut mailboxes = InMemoryMailboxCache::new();

    // alice has a known write relay.
    put_write_mailbox(&mut mailboxes, padded_pubkey("alice"), "wss://alice-relay/");

    // Register a tailing interest for alice.
    nmp_core::subs::replace_test_interest(&mut lc, tailing_interest(1, &["alice"]));

    // Compile: alice's relay must receive a REQ.
    let frames1 = lc.recompile_and_diff(&mailboxes).expect("initial compile");
    let req_relays1 = req_relays(&frames1);
    assert!(
        req_relays1.contains(&"wss://alice-relay/"),
        "initial compile must REQ alice's relay; got {req_relays1:?}"
    );
    assert_eq!(lc.compile_count(), 1);

    // Wire carol's mailbox so the recompile finds a route.
    put_write_mailbox(&mut mailboxes, padded_pubkey("carol"), "wss://carol-relay/");

    // Expand the interest to cover carol too (production view rebuild equivalent).
    nmp_core::subs::replace_test_interest(&mut lc, tailing_interest(1, &["alice", "carol"]));

    // Fire the A11 FollowListChanged trigger — the canonical kind:3 rewire signal.
    lc.enqueue_trigger(CompileTrigger::FollowListChanged {
        account_id: AccountId(padded_pubkey("alice")),
        new_follows: vec![padded_pubkey("carol")],
    });

    let frames2 = lc.drain_tick(&mailboxes);
    assert_eq!(
        lc.compile_count(),
        2,
        "drain_tick must recompile on FollowListChanged trigger"
    );

    let req_relays2 = req_relays(&frames2);
    assert!(
        req_relays2.contains(&"wss://carol-relay/"),
        "after follow-list update, recompile must REQ carol's relay; frames={frames2:?}"
    );

    // Idempotence: empty-inbox tick must emit no frames.
    let frames3 = lc.drain_tick(&mailboxes);
    assert!(
        frames3.is_empty(),
        "empty-inbox tick must emit zero frames; got {frames3:?}"
    );
    assert_eq!(
        lc.compile_count(),
        2,
        "empty-inbox tick must not bump compile count"
    );
}
