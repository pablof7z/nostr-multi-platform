//! T142 integration tests — drain_tick() actor-idle-loop driver.
//!
//! Proves that the subscription lifecycle tick machinery wired in T142 is
//! correct end-to-end at the public `nmp_core::subs` API boundary:
//!
//! 1. `t142_actor_idle_loop_drains_tick` — a queued trigger produces wire
//!    frames when drain_tick is called (simulating what the actor idle loop does
//!    via `Kernel::drain_lifecycle_tick()`).
//! 2. `t142_follow_list_update_produces_wire_frames_e2e` — full path: follow
//!    interest registered + FollowListChanged trigger + drain → REQ frames on
//!    the author's relay.
//! 3. `t142_empty_tick_no_recompile` — idle tick with no triggers → no compile
//!    and no frames (D8 zero-cost no-op invariant).
//!
//! Design: these tests exercise the public `SubscriptionLifecycle` interface at
//! the integration boundary, which is the same state machine that
//! `Kernel::drain_lifecycle_tick()` drives internally. The actor unit tests
//! (in `nmp-core/src/actor/tests.rs`) cover the `wire_frames_to_outbound`
//! conversion; these tests cover the `drain_tick` trigger-to-frame pipeline.

use nmp_core::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};
use nmp_core::subs::{AccountId, CompileTrigger, InvalidateReason, SubscriptionLifecycle, WireFrame};

fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect::<String>().to_lowercase()
}

fn interest_for(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey(author)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

fn cache_for(author: &str, relay: &str) -> InMemoryMailboxCache {
    let mut c = InMemoryMailboxCache::new();
    c.put(
        pubkey(author),
        MailboxSnapshot {
            write_relays: vec![relay.to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    c
}

// ─── Test 1 — idle loop drains tick ─────────────────────────────────────────

/// The actor idle loop calls drain_tick at each iteration. This test proves
/// that a trigger enqueued before the tick produces wire frames (the same
/// path the actor uses via `Kernel::drain_lifecycle_tick()`).
#[test]
fn t142_actor_idle_loop_drains_tick() {
    let mut lifecycle = SubscriptionLifecycle::new();
    lifecycle.registry_mut().push(interest_for(1, "alice"));
    lifecycle.set_selection_budget(usize::MAX, usize::MAX);

    let mailboxes = cache_for("alice", "wss://t142-test.example");

    // Simulate the actor idle loop: enqueue a trigger then drain.
    lifecycle.enqueue_trigger(CompileTrigger::InvalidateCompile {
        reason: InvalidateReason::TestForceRecompile,
    });

    // This is the call the actor's idle loop makes (via drain_lifecycle_tick on Kernel).
    let frames = lifecycle.drain_tick(&mailboxes);

    let req_count = frames.iter().filter(|f| matches!(f, WireFrame::Req { .. })).count();
    assert!(
        req_count > 0,
        "queued trigger with registered interests must produce REQ frames (got {req_count})",
    );
}

// ─── Test 2 — follow list update produces wire frames end-to-end ─────────────

/// Full path: follow interest registered + FollowListChanged trigger (A11)
/// enqueued → drain_tick → REQ frames emitted for the author's relay.
#[test]
fn t142_follow_list_update_produces_wire_frames_e2e() {
    let mut lifecycle = SubscriptionLifecycle::new();
    let author = pubkey("bob");

    lifecycle.registry_mut().push(interest_for(2, "bob"));
    lifecycle.set_selection_budget(usize::MAX, usize::MAX);

    let mailboxes = cache_for("bob", "wss://bob-relay.example");

    // Enqueue the A11 trigger (follow list changed — simulates kind:3 ingested).
    lifecycle.enqueue_trigger(CompileTrigger::FollowListChanged {
        account_id: AccountId("test-account".to_string()),
        new_follows: vec![author],
    });

    let frames = lifecycle.drain_tick(&mailboxes);

    let req_urls: Vec<String> = frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();

    assert!(
        req_urls.iter().any(|url| url == "wss://bob-relay.example"),
        "FollowListChanged + drain_tick must produce REQ for bob's relay (got {:?})",
        req_urls,
    );
}

// ─── Test 3 — empty tick no recompile (D8 zero-cost no-op invariant) ─────────

/// With no interests registered AND no triggers enqueued, drain_tick() must
/// return no frames and must NOT invoke the planner (compile count unchanged).
///
/// D8 empty-registry invariant: the actor idle loop calls drain_tick() on every
/// tick. When no UI has claimed any interest (cold-start, background, or between
/// sessions), the cost of that call must be a single `inbox.is_empty()` check
/// — zero allocation, zero compile pass. This is the most common case.
#[test]
fn t142_empty_tick_no_recompile() {
    let mut lifecycle = SubscriptionLifecycle::new();
    // No interests registered — empty registry, not just empty inbox.

    let mailboxes = cache_for("carol", "wss://carol-relay.example");
    let before = lifecycle.compile_count();

    // NO trigger enqueued — this must be a zero-cost no-op.
    let frames = lifecycle.drain_tick(&mailboxes);

    assert!(
        frames.is_empty(),
        "empty inbox tick must return no frames (got {})",
        frames.len(),
    );
    assert_eq!(
        lifecycle.compile_count(),
        before,
        "empty inbox tick must not increment compile count (before={before}, after={})",
        lifecycle.compile_count(),
    );
}
