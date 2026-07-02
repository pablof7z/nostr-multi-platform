//! Test 5 — auth_required_for_read_flow
//!
//! Scenario:
//!   1. Build a SubscriptionLifecycle with an interest for alice.
//!   2. AUTH challenge arrives BEFORE the first compile (relay is auth-paused).
//!   3. Compile: REQs targeting the paused relay are withheld by the auth-gate.
//!   4. Assert zero REQs on the wire.
//!   5. AUTH completes (Authenticated): pending REQs are flushed.
//!   6. Assert the flushed REQs target the expected relay.
//!
//! This is the M5 NIP-42 relay auth contract.  The auth-gate (subs/auth_gate.rs)
//! intercepts REQs during `recompile_and_diff` / `drain_tick` when a relay is
//! in ChallengeReceived state.  The flush happens when `handle_auth_state_change`
//! transitions to Authenticated.  The key timing: the challenge must arrive
//! BEFORE the compile so the `partition()` path captures the REQs.

use crate::support::{padded_pubkey, put_write_mailbox};

#[test]
fn auth_required_for_read_flow() {
    use nmp_core::subs::{RelayAuthState, SubscriptionLifecycle, WireFrame};
    use nmp_planner::{
        InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
        LogicalInterest,
    };
    use std::collections::BTreeSet;

    let relay_url = "wss://auth-relay/";

    let mut lc = SubscriptionLifecycle::new();
    let mut mailboxes = InMemoryMailboxCache::new();
    put_write_mailbox(&mut mailboxes, padded_pubkey("alice"), relay_url);

    nmp_core::subs::replace_test_interest(
        &mut lc,
        LogicalInterest {
            id: InterestId(1),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: [padded_pubkey("alice")]
                    .into_iter()
                    .collect::<BTreeSet<_>>(),
                kinds: [1u32].into_iter().collect(),
                ..Default::default()
            },
            hints: vec![],
            lifecycle: InterestLifecycle::Tailing,
            is_indexer_discovery: false,
        },
    );

    // Phase 1: AUTH challenge arrives BEFORE the first compile.
    // This puts the relay into the paused state so recompile_and_diff routes
    // the produced REQs through the auth-gate partition path.
    let _pre =
        lc.handle_auth_state_change(relay_url.to_string(), RelayAuthState::ChallengeReceived);

    // Phase 2: Compile while auth-paused.
    // REQs targeting the paused relay must be captured in the pending buffer,
    // not returned to the caller (zero wire frames for this relay).
    let frames_paused = lc
        .recompile_and_diff(&mailboxes)
        .expect("auth-paused compile");
    let reqs_to_paused: Vec<_> = frames_paused
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url: u, .. } if u == relay_url))
        .collect();
    assert!(
        reqs_to_paused.is_empty(),
        "REQs to NIP-42 auth-paused relay must be withheld from the wire; got {} frame(s)",
        reqs_to_paused.len()
    );

    // Phase 3: AUTH completes — pending REQs must be flushed to the wire.
    let flush_frames =
        lc.handle_auth_state_change(relay_url.to_string(), RelayAuthState::Authenticated);
    let reqs_flushed: Vec<_> = flush_frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url: u, .. } if u == relay_url))
        .collect();
    assert!(
        !reqs_flushed.is_empty(),
        "Authenticated transition must flush buffered REQs to the relay; got 0"
    );
}
