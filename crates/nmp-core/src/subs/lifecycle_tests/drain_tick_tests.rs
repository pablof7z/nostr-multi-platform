//! T142 unit tests — `drain_tick()` actor-idle-loop driver: empty-inbox
//! no-op, trigger-driven REQ emission, the auth-gate pause/flush side
//! effect, and per-tick compile coalescing (D8).
use super::*;
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};

/// T142-U1: Empty inbox tick returns no frames and does not compile.
/// Proves the zero-cost no-op guarantee from the spec §1 point 3.
#[test]
fn drain_tick_empty_inbox_returns_no_frames() {
    let mut l = SubscriptionLifecycle::new();
    // No interests, no triggers — inbox is empty.
    let mailboxes = InMemoryMailboxCache::new();
    let frames = l.drain_tick(&mailboxes);
    assert!(frames.is_empty(), "empty inbox must return no frames");
    assert_eq!(
        l.compile_count(),
        0,
        "empty inbox must not trigger a compile"
    );
}

/// T142-U2: A FollowListChanged trigger with follow interests → REQ frames.
/// Proves A11 trigger + follow interests → wire frames returned.
#[test]
fn drain_tick_follow_list_changed_emits_req_frames() {
    let mut l = SubscriptionLifecycle::new();
    let author = pubkey("alice");
    l.set_selection_budget(usize::MAX, usize::MAX);

    // Register a follow interest.
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [author.clone()].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    };
    push_legacy(l.registry_mut(), interest);

    // Set up mailbox so the author routes to a relay.
    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        author.clone(),
        MailboxSnapshot {
            write_relays: vec!["wss://drain-test.example".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    // Enqueue a FollowListChanged trigger (A11).
    l.enqueue_trigger(CompileTrigger::FollowListChanged {
        account_id: AccountId("test-account".to_string()),
        new_follows: vec![author],
    });

    let frames = l.drain_tick(&mailboxes);
    let req_count = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .count();
    assert!(
        req_count > 0,
        "FollowListChanged trigger with interests must emit REQ frames (got {req_count})"
    );
}

/// T142-U3: RelayAuthStateChanged → AuthGate state applied before compile.
/// Proves that the auth-state side-effect lands in the AuthGate before the
/// compile pass runs (spec §1 point 2).
#[test]
fn drain_tick_relay_auth_changed_applies_side_effect() {
    let mut l = SubscriptionLifecycle::new();
    let relay_url = "wss://auth-test.example".to_string();

    // Before the trigger: relay is NOT paused.
    assert!(
        !l.is_auth_paused_for_url(&relay_url),
        "relay should not be paused initially"
    );

    // Enqueue a ChallengeReceived transition — should pause the relay.
    l.enqueue_trigger(CompileTrigger::RelayAuthStateChanged {
        url: relay_url.clone(),
        state: RelayAuthState::ChallengeReceived,
    });

    let mailboxes = InMemoryMailboxCache::new();
    let _frames = l.drain_tick(&mailboxes);

    // After drain_tick the side effect must be applied.
    assert!(
        l.is_auth_paused_for_url(&relay_url),
        "relay must be paused after ChallengeReceived side effect"
    );
}

/// `RelayAuthStateChanged{Authenticated}` via drain_tick must flush buffered REQs.
///
/// Production auth flushes go through `handle_auth_state_change` (direct path
/// in `ingest/auth_handlers.rs`). This test covers the trigger path so that if
/// `RelayAuthStateChanged` is ever enqueued as a trigger the pending REQs are
/// returned rather than silently dropped.
#[test]
fn drain_tick_authenticated_flushes_pending_reqs() {
    use crate::subs::trigger::RelayAuthState;
    let mut l = SubscriptionLifecycle::new();
    let relay_url = "wss://auth-flush.example".to_string();
    let mailboxes = InMemoryMailboxCache::new();

    // Step 1: make the relay the single app relay and register an interest
    // so the compile routes a REQ to it.
    l.set_app_relays(vec![relay_url.clone()]);
    push_legacy(l.registry_mut(), follow(1, "aa"));

    // Step 2: pause the relay (ChallengeReceived) and compile — REQs get buffered.
    l.enqueue_trigger(CompileTrigger::RelayAuthStateChanged {
        url: relay_url.clone(),
        state: RelayAuthState::ChallengeReceived,
    });
    let paused_frames = l.drain_tick(&mailboxes);
    let paused_reqs = paused_frames
        .iter()
        .filter(
            |f| matches!(f, WireFrame::Req { relay_url: u, .. } if u == "wss://auth-flush.example"),
        )
        .count();
    assert_eq!(
        paused_reqs, 0,
        "REQs to a paused relay must not appear in drain_tick output; got {paused_reqs}"
    );

    // Step 3: authenticate — pending REQs must be flushed in the same tick.
    l.enqueue_trigger(CompileTrigger::RelayAuthStateChanged {
        url: relay_url.clone(),
        state: RelayAuthState::Authenticated,
    });
    let flushed_frames = l.drain_tick(&mailboxes);
    let flushed_reqs = flushed_frames
        .iter()
        .filter(
            |f| matches!(f, WireFrame::Req { relay_url: u, .. } if u == "wss://auth-flush.example"),
        )
        .count();
    assert!(
        flushed_reqs > 0,
        "Authenticated trigger via drain_tick must flush buffered REQs; got {flushed_reqs}"
    );
}

/// T142-U4: N triggers in one tick → exactly 1 compile (D8 coalescing).
/// Proves the per-tick discipline: N triggers → at most 1 compile.
#[test]
fn drain_tick_coalesces_multiple_triggers() {
    let mut l = SubscriptionLifecycle::new();
    let mailboxes = InMemoryMailboxCache::new();
    let baseline = l.compile_count();

    // Enqueue 10 triggers within the same tick.
    for _ in 0..10 {
        l.enqueue_trigger(CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::TestForceRecompile,
        });
    }

    let _frames = l.drain_tick(&mailboxes);

    assert_eq!(
        l.compile_count(),
        baseline + 1,
        "10 triggers in one tick must coalesce into exactly 1 compile (got {} compiles)",
        l.compile_count() - baseline,
    );
}
