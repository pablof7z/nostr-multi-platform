//! M8-subs — subscription lifecycle integration tests (Task #46).
//!
//! Pins the eight contracts in `docs/plan/m8-subscription-lifecycle.md` §6:
//!
//! 1. `compile_plan_to_wire_frames_emits_one_req_per_sub_shape`
//! 2. `plan_diff_closes_removed_subs_and_opens_added_subs`
//! 3. `reconnect_replays_current_plan_without_recompile`
//! 4. `trigger_inbox_coalesces_within_one_tick`
//! 5. `send_path_defers_outbound_when_pool_disconnected`
//! 6. `auth_paused_relay_holds_reqs_until_authenticated`
//!
//! Design: `docs/design/subscription-compilation/recompilation.md` §4.
//! Doctrine: D3 (routing automatic), D4 (single-writer registry), D6 (errors
//! internal), D7 (pool reports, actor decides), D8 (per-tick coalesce).

use std::collections::BTreeSet;

use nmp_core::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest,
};
use nmp_core::subs::{
    plan_diff, CompileTrigger, ConnectionPool, InMemoryPool, InvalidateReason, PoolSendOutcome,
    RelayAuthState, SubscriptionLifecycle, WireFrame,
};

// ─── Helpers ────────────────────────────────────────────────────────────────

fn pubkey(seed: &str) -> String {
    format!("{seed:0>64}")
        .chars()
        .take(64)
        .collect::<String>()
        .to_lowercase()
}

fn interest(id: u64, authors: &[&str], lifecycle: InterestLifecycle) -> LogicalInterest {
    let shape = InterestShape {
        authors: authors.iter().map(|a| pubkey(a)).collect::<BTreeSet<_>>(),
        kinds: [1u32, 6u32].into_iter().collect(),
        ..Default::default()
    };
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape,
        hints: Vec::new(),
        lifecycle,
    }
}

fn mailboxes_for(
    authors: &[&str],
    write_relays: &[&str],
) -> Vec<(String, nmp_core::planner::MailboxSnapshot)> {
    use nmp_core::planner::MailboxSnapshot;
    authors
        .iter()
        .map(|a| {
            (
                pubkey(a),
                MailboxSnapshot {
                    write_relays: write_relays.iter().map(|r| r.to_string()).collect(),
                    read_relays: vec![],
                    both_relays: vec![],
                },
            )
        })
        .collect()
}

/// T132 helper — build a `MailboxCache` populated with one author/relay pair.
///
/// The lifecycle no longer owns the cache; callers construct one (in tests)
/// and pass it into `recompile_and_diff` / `drain_tick`. In production the
/// kernel passes its `KernelMailboxes` adapter, but here the
/// `InMemoryMailboxCache` is the polymorphism seam.
fn cache_with(author: &str, write_relays: &[&str]) -> InMemoryMailboxCache {
    use nmp_core::planner::MailboxSnapshot;
    let mut c = InMemoryMailboxCache::new();
    c.put(
        pubkey(author),
        MailboxSnapshot {
            write_relays: write_relays.iter().map(|r| r.to_string()).collect(),
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    c
}

// ─── Test 1 — wire-frame emission ────────────────────────────────────────────

#[test]
fn compile_plan_to_wire_frames_emits_one_req_per_sub_shape() {
    use nmp_core::planner::{InMemoryMailboxCache, SubscriptionCompiler};

    let mut cache = InMemoryMailboxCache::new();
    for (pk, mb) in mailboxes_for(&["alice", "bob"], &["wss://relay.damus.io"]) {
        cache.put(pk, mb);
    }
    let indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let interests = vec![
        interest(1, &["alice"], InterestLifecycle::Tailing),
        interest(2, &["bob"], InterestLifecycle::Tailing),
    ];

    let plan = compiler.compile(&interests).expect("compile");
    let frames: Vec<WireFrame> = plan_diff(None, Some(&plan), &interests);

    // Each SubShape becomes exactly one REQ frame (no CLOSEs because no prior plan).
    let req_frames: Vec<_> = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .collect();
    let close_frames: Vec<_> = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Close { .. }))
        .collect();

    let total_shapes: usize = plan.per_relay.values().map(|p| p.sub_shapes.len()).sum();
    assert_eq!(
        req_frames.len(),
        total_shapes,
        "one REQ per SubShape (got {} REQs for {} shapes)",
        req_frames.len(),
        total_shapes,
    );
    assert!(close_frames.is_empty(), "no CLOSEs on initial plan");

    // Each REQ frame is well-formed: ["REQ", sub_id, filter_json].
    for frame in &req_frames {
        if let WireFrame::Req {
            sub_id,
            filter_json,
            ..
        } = frame
        {
            assert!(!sub_id.is_empty(), "sub_id non-empty");
            assert!(filter_json.starts_with('{'), "filter is a JSON object");
        }
    }
}

// ─── Test 2 — plan diff ──────────────────────────────────────────────────────

#[test]
fn plan_diff_closes_removed_subs_and_opens_added_subs() {
    use nmp_core::planner::{InMemoryMailboxCache, SubscriptionCompiler};

    let mut cache = InMemoryMailboxCache::new();
    for (pk, mb) in mailboxes_for(&["alice", "bob", "carol"], &["wss://relay.damus.io"]) {
        cache.put(pk, mb);
    }
    let indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let interests_a = vec![
        interest(1, &["alice"], InterestLifecycle::Tailing),
        interest(2, &["bob"], InterestLifecycle::Tailing),
    ];
    let plan_a = compiler.compile(&interests_a).expect("compile A");

    let interests_b = vec![
        interest(1, &["alice"], InterestLifecycle::Tailing),
        interest(3, &["carol"], InterestLifecycle::Tailing),
    ];
    let plan_b = compiler.compile(&interests_b).expect("compile B");

    let diff = plan_diff(Some(&plan_a), Some(&plan_b), &interests_b);

    // Expect at least one CLOSE (bob's sub) and at least one REQ (carol's sub).
    let close_count = diff
        .iter()
        .filter(|f| matches!(f, WireFrame::Close { .. }))
        .count();
    let req_count = diff
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .count();

    assert!(
        close_count >= 1,
        "removed sub-shape produces CLOSE (got {close_count})"
    );
    assert!(
        req_count >= 1,
        "added sub-shape produces REQ (got {req_count})"
    );

    // Idempotence: diff(B, B) is empty.
    let noop = plan_diff(Some(&plan_b), Some(&plan_b), &interests_b);
    assert!(noop.is_empty(), "identical plans produce no frames");
}

// ─── Test 3 — reconnect replay (A5) ──────────────────────────────────────────

#[test]
fn reconnect_replays_current_plan_without_recompile() {
    let mut lifecycle = SubscriptionLifecycle::new();
    let interests = vec![interest(1, &["alice"], InterestLifecycle::Tailing)];
    for i in &interests {
        lifecycle.registry_mut().push(i.clone());
    }

    // Initial compile + emit. T132: caller-owned mailbox cache.
    let mailboxes = cache_with("alice", &["wss://relay.damus.io"]);
    let _initial = lifecycle
        .recompile_and_diff(&mailboxes)
        .expect("initial compile");
    let baseline_compile_count = lifecycle.compile_count();

    // Trigger reconnect — must replay current plan to that relay without
    // bumping compile_count.
    let replay = lifecycle.handle_reconnect("wss://relay.damus.io".to_string());

    assert_eq!(
        lifecycle.compile_count(),
        baseline_compile_count,
        "reconnect must not invoke planner",
    );
    let req_frames: Vec<_> = replay
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .collect();
    assert!(!req_frames.is_empty(), "reconnect replays at least one REQ");
    // All replay frames target the reconnected relay.
    for frame in &replay {
        if let WireFrame::Req { relay_url, .. } = frame {
            assert_eq!(relay_url, "wss://relay.damus.io");
        }
    }
}

// ─── Test 4 — trigger coalescing ─────────────────────────────────────────────

#[test]
fn trigger_inbox_coalesces_within_one_tick() {
    let mut lifecycle = SubscriptionLifecycle::new();
    lifecycle
        .registry_mut()
        .push(interest(1, &["alice"], InterestLifecycle::Tailing));
    let mailboxes = cache_with("alice", &["wss://relay.damus.io"]);

    let baseline = lifecycle.compile_count();

    // Enqueue 50 triggers within one tick boundary.
    for _ in 0..50 {
        lifecycle.enqueue_trigger(CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::TestForceRecompile,
        });
    }

    // One tick drain coalesces them all into one compile pass.
    let _frames = lifecycle.drain_tick(&mailboxes);

    assert_eq!(
        lifecycle.compile_count(),
        baseline + 1,
        "50 triggers in one tick → exactly one compile (got {})",
        lifecycle.compile_count() - baseline,
    );

    // Subsequent tick with empty inbox does not compile.
    let _empty = lifecycle.drain_tick(&mailboxes);
    assert_eq!(
        lifecycle.compile_count(),
        baseline + 1,
        "empty inbox tick does not compile",
    );
}

// ─── Test 5 — send-path defer-on-disconnect ──────────────────────────────────

#[test]
fn send_path_defers_outbound_when_pool_disconnected() {
    let mut pool = InMemoryPool::new();
    // No relay registered → disconnected.

    let outcome = pool.send("wss://relay.damus.io", "[\"REQ\",\"x\",{}]".to_string());
    assert!(
        matches!(outcome, PoolSendOutcome::Deferred),
        "send to unregistered relay defers (got {outcome:?})",
    );
    assert_eq!(pool.deferred_count("wss://relay.damus.io"), 1);

    // Connect the relay; drained queue produces sent count = 1.
    pool.mark_connected("wss://relay.damus.io");
    let drained = pool.drain_deferred("wss://relay.damus.io");
    assert_eq!(
        drained.len(),
        1,
        "reconnect drains exactly the deferred frame"
    );

    // Now a normal send goes through immediately.
    let outcome = pool.send("wss://relay.damus.io", "[\"REQ\",\"y\",{}]".to_string());
    assert!(
        matches!(outcome, PoolSendOutcome::Sent),
        "send to connected relay succeeds immediately",
    );
}

// ─── Test 6 — auth-paused gate (A9) ──────────────────────────────────────────

#[test]
fn auth_paused_relay_holds_reqs_until_authenticated() {
    let mut lifecycle = SubscriptionLifecycle::new();
    lifecycle
        .registry_mut()
        .push(interest(1, &["alice"], InterestLifecycle::Tailing));
    let mailboxes = cache_with("alice", &["wss://relay.damus.io"]);

    // Auth challenge arrives BEFORE the first compile.
    lifecycle.enqueue_trigger(CompileTrigger::RelayAuthStateChanged {
        url: "wss://relay.damus.io".to_string(),
        state: RelayAuthState::ChallengeReceived,
    });
    let _drain = lifecycle.drain_tick(&mailboxes);

    // Now compile — REQs that would target the paused relay must be withheld.
    let frames_during_pause = lifecycle.recompile_and_diff(&mailboxes).expect("compile");
    let reqs_to_paused: Vec<_> = frames_during_pause
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url, .. } if relay_url == "wss://relay.damus.io"))
        .collect();
    assert!(
        reqs_to_paused.is_empty(),
        "REQs to auth-paused relay are withheld (got {} frames)",
        reqs_to_paused.len(),
    );

    // Auth completes; pending REQs flush.
    let on_auth = lifecycle.handle_auth_state_change(
        "wss://relay.damus.io".to_string(),
        RelayAuthState::Authenticated,
    );
    let reqs_after_auth: Vec<_> = on_auth
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url, .. } if relay_url == "wss://relay.damus.io"))
        .collect();
    assert!(
        !reqs_after_auth.is_empty(),
        "Authenticated transition flushes pending REQs (got {} frames)",
        reqs_after_auth.len(),
    );
}
