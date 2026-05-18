//! Lifecycle smoke, `apply_selection` wiring, dead-relay exclusion, and
//! `drain_tick` actor-idle-loop driver tests.
//!
//! Relocated verbatim out of `subs/mod.rs`'s inline `mod tests` (file-size
//! gate, NMP #169). No assertion, fixture, or test body was changed — only
//! the host module moved. `use super::*;` resolves to the `subs` module just
//! as it did when this lived inside `mod.rs`'s `mod tests`.

use super::*;
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};

fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

/// Single-author follow interest (kind:1 timeline).
fn follow(id: u64, author: &str) -> LogicalInterest {
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
    }
}

#[test]
fn empty_lifecycle_starts_with_zero_compiles() {
    let l = SubscriptionLifecycle::new();
    assert_eq!(l.compile_count(), 0);
    assert!(l.current_plan.is_none());
}

#[test]
fn empty_tick_does_not_compile() {
    let mut l = SubscriptionLifecycle::new();
    let mailboxes = InMemoryMailboxCache::new();
    let frames = l.drain_tick(&mailboxes);
    assert!(frames.is_empty());
    assert_eq!(l.compile_count(), 0);
}

// ─── apply_selection wiring ──────────────────────────────────────────────

/// With 10 follows each declaring a unique write relay (no shared
/// coverage), the naive plan would carry 10 relay entries. Bound
/// `max_connections = 5` to force the greedy selector to actually prune
/// — proving `apply_selection` is wired into `recompile_and_diff` (not a
/// no-op).
#[test]
fn recompile_caps_per_relay_at_max_connections() {
    let mut l = SubscriptionLifecycle::new();
    l.set_app_relays(vec!["wss://app.example".to_string()]);
    // Tighten the budget so the test is independent of the default
    // (which would not prune at only 10 follows).
    let max_connections: usize = 5;
    l.set_selection_budget(max_connections, 2);

    let mut mailboxes = InMemoryMailboxCache::new();
    for i in 0..10u32 {
        let author_seed = format!("aa{i:02}");
        let relay = format!("wss://r{i:02}.example");
        mailboxes.put(
            pubkey(&author_seed),
            MailboxSnapshot {
                write_relays: vec![relay],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        l.registry_mut().push(follow(u64::from(i) + 1, &author_seed));
    }

    let _frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let plan = l.current_plan.as_ref().expect("plan present");
    assert!(
        plan.per_relay.len() <= max_connections,
        "per_relay.len() = {} must be ≤ max_connections = {}",
        plan.per_relay.len(),
        max_connections,
    );
}

/// A relay served by the naive plan on the first recompile drops out of
/// the second when the selection budget is tightened. The wire-emitter
/// diff MUST emit a CLOSE for every shape that was on the now-dropped
/// relay (the diff iterates prior `per_relay` and CLOSEs any sub_id not
/// in the next set — verifying that relays disappearing under selection
/// are handled cleanly).
#[test]
fn dropped_relay_emits_close_on_next_recompile() {
    let mut l = SubscriptionLifecycle::new();
    // First compile with a generous budget — every relay survives.
    l.set_selection_budget(usize::MAX, usize::MAX);

    let mut mailboxes = InMemoryMailboxCache::new();
    for i in 0..3u32 {
        let author_seed = format!("bb{i:02}");
        let relay = format!("wss://drop{i:02}.example");
        mailboxes.put(
            pubkey(&author_seed),
            MailboxSnapshot {
                write_relays: vec![relay],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        l.registry_mut().push(follow(u64::from(i) + 1, &author_seed));
    }

    let first = l.recompile_and_diff(&mailboxes).expect("first compile");
    let req_relays: std::collections::BTreeSet<String> = first
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        req_relays.len(),
        3,
        "first compile must REQ all 3 relays; got {req_relays:?}",
    );

    // Tighten the budget so 2 relays must be dropped on the next compile.
    l.set_selection_budget(1, 1);
    let second = l.recompile_and_diff(&mailboxes).expect("second compile");

    let plan = l.current_plan.as_ref().expect("plan present");
    assert_eq!(
        plan.per_relay.len(),
        1,
        "selection budget = 1 → exactly one relay survives; got {}",
        plan.per_relay.len(),
    );
    let surviving: std::collections::BTreeSet<String> =
        plan.per_relay.keys().cloned().collect();

    let closes: std::collections::BTreeSet<String> = second
        .iter()
        .filter_map(|f| match f {
            WireFrame::Close { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();
    // Every relay that disappeared must have at least one CLOSE.
    let expected_dropped: std::collections::BTreeSet<String> =
        req_relays.difference(&surviving).cloned().collect();
    assert_eq!(expected_dropped.len(), 2, "two relays must have been dropped");
    for dropped in &expected_dropped {
        assert!(
            closes.contains(dropped),
            "wire-emitter diff must CLOSE the dropped relay {dropped}; got {closes:?}",
        );
    }
}

/// `set_indexer_relays` mutates the lifecycle's stored set and the next
/// `recompile_and_diff` threads the override into the compiler.
///
/// We do NOT assert via the resulting plan because the case-D cold-start
/// path produces a wildcard-author sub-shape, which `apply_selection`
/// (now wired into the recompile path) deliberately drops (see
/// `selection.rs` §"Wildcard-author sub-shapes" — relays whose only
/// contribution is wildcard coverage are dropped). Instead, this test
/// (a) verifies the setter mutated the field, and (b) verifies the
/// recompile path still consumes the field cleanly. The compile-time
/// case-D cold-start behaviour is covered by
/// `planner::compiler::partition::case_d_no_author::tests::case_d_cold_start_falls_through_to_indexer`.
#[test]
fn set_indexer_relays_is_reflected_in_next_recompile() {
    let mut l = SubscriptionLifecycle::new();
    assert_eq!(
        l.indexer_relays(),
        &["wss://purplepag.es".to_string()],
        "default indexer set is purplepag.es",
    );

    l.set_indexer_relays(vec!["wss://sentinel-indexer.example".to_string()]);
    assert_eq!(
        l.indexer_relays(),
        &["wss://sentinel-indexer.example".to_string()],
        "setter must replace the indexer set",
    );

    // Recompile with an empty registry should succeed (no-op compile)
    // and increment the compile counter — proving the new indexer set
    // is not poison input to the recompile path.
    let mailboxes = InMemoryMailboxCache::new();
    let prior = l.compile_count();
    let _ = l.recompile_and_diff(&mailboxes).expect("compile");
    assert_eq!(
        l.compile_count(),
        prior + 1,
        "recompile must run with the new indexer set installed",
    );
    // And the value must still be the override (not reset by recompile).
    assert_eq!(
        l.indexer_relays(),
        &["wss://sentinel-indexer.example".to_string()],
    );
}

// ─── dead-relay exclusion ────────────────────────────────────────────────

/// An author who declares two write relays should land on the alive one
/// when the other is marked dead. The dead relay must not appear in the
/// resulting plan; the alive one must.
#[test]
fn dead_relay_excluded_from_next_recompile() {
    let mut l = SubscriptionLifecycle::new();
    l.set_selection_budget(usize::MAX, usize::MAX);

    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        pubkey("cc01"),
        MailboxSnapshot {
            write_relays: vec![
                "wss://alive.example".to_string(),
                "wss://dead.example".to_string(),
            ],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    l.registry_mut().push(follow(1, "cc01"));

    // First compile: both relays present.
    let _ = l.recompile_and_diff(&mailboxes).expect("first compile");
    let before = l.current_plan.as_ref().expect("plan").per_relay.clone();
    assert!(before.contains_key("wss://alive.example"));
    assert!(before.contains_key("wss://dead.example"));

    // Mark dead.example as dead and recompile.
    assert!(l.mark_relay_dead("wss://dead.example".to_string()));
    let _ = l.recompile_and_diff(&mailboxes).expect("second compile");
    let after = &l.current_plan.as_ref().expect("plan").per_relay;
    assert!(
        after.contains_key("wss://alive.example"),
        "alive relay must still serve cc01"
    );
    assert!(
        !after.contains_key("wss://dead.example"),
        "dead relay must not appear in the plan"
    );
}

/// An author whose ENTIRE declared write set is dead falls out of the
/// plan entirely (no candidate relay to route to). When a relay becomes
/// alive again, the next recompile routes the author back to it.
#[test]
fn fully_dead_author_returns_when_relay_alive_again() {
    let mut l = SubscriptionLifecycle::new();
    l.set_selection_budget(usize::MAX, usize::MAX);

    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        pubkey("dd01"),
        MailboxSnapshot {
            write_relays: vec!["wss://only.example".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    l.registry_mut().push(follow(1, "dd01"));

    // Compile, kill, recompile.
    let _ = l.recompile_and_diff(&mailboxes).expect("compile 1");
    assert!(l
        .current_plan
        .as_ref()
        .unwrap()
        .per_relay
        .contains_key("wss://only.example"));

    l.mark_relay_dead("wss://only.example".to_string());
    let _ = l.recompile_and_diff(&mailboxes).expect("compile 2");
    assert!(
        l.current_plan.as_ref().unwrap().per_relay.is_empty(),
        "all relays dead → empty plan"
    );

    // Resurrect.
    assert!(l.mark_relay_alive(&"wss://only.example".to_string()));
    let _ = l.recompile_and_diff(&mailboxes).expect("compile 3");
    assert!(l
        .current_plan
        .as_ref()
        .unwrap()
        .per_relay
        .contains_key("wss://only.example"));
}

/// Toggling a relay's state fires the `RelayHealthChanged` trigger.
/// Marking an already-dead relay dead (or already-alive alive) is a no-op
/// and does NOT enqueue a redundant trigger.
#[test]
fn mark_dead_idempotent_and_fires_trigger_only_on_change() {
    let mut l = SubscriptionLifecycle::new();
    assert!(l.mark_relay_dead("wss://x.example".to_string()));
    assert!(!l.mark_relay_dead("wss://x.example".to_string())); // already dead
    assert!(l.mark_relay_alive(&"wss://x.example".to_string()));
    assert!(!l.mark_relay_alive(&"wss://x.example".to_string())); // already alive
    assert!(l.dead_relays().is_empty());
}

// ─── T142 unit tests — drain_tick() actor-idle-loop driver ──────────────

/// T142-U1: Empty inbox tick returns no frames and does not compile.
/// Proves the zero-cost no-op guarantee from the spec §1 point 3.
#[test]
fn drain_tick_empty_inbox_returns_no_frames() {
    let mut l = SubscriptionLifecycle::new();
    // No interests, no triggers — inbox is empty.
    let mailboxes = InMemoryMailboxCache::new();
    let frames = l.drain_tick(&mailboxes);
    assert!(frames.is_empty(), "empty inbox must return no frames");
    assert_eq!(l.compile_count(), 0, "empty inbox must not trigger a compile");
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
    };
    l.registry_mut().push(interest);

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
    let req_count = frames.iter().filter(|f| matches!(f, WireFrame::Req { .. })).count();
    assert!(req_count > 0, "FollowListChanged trigger with interests must emit REQ frames (got {req_count})");
}

/// T142-U3: RelayAuthStateChanged → AuthGate state applied before compile.
/// Proves that the auth-state side-effect lands in the AuthGate before the
/// compile pass runs (spec §1 point 2).
#[test]
fn drain_tick_relay_auth_changed_applies_side_effect() {
    let mut l = SubscriptionLifecycle::new();
    let relay_url = "wss://auth-test.example".to_string();

    // Before the trigger: relay is NOT paused.
    assert!(!l.is_auth_paused_for_url(&relay_url), "relay should not be paused initially");

    // Enqueue a ChallengeReceived transition — should pause the relay.
    l.enqueue_trigger(CompileTrigger::RelayAuthStateChanged {
        url: relay_url.clone(),
        state: RelayAuthState::ChallengeReceived,
    });

    let mailboxes = InMemoryMailboxCache::new();
    let _frames = l.drain_tick(&mailboxes);

    // After drain_tick the side effect must be applied.
    assert!(l.is_auth_paused_for_url(&relay_url), "relay must be paused after ChallengeReceived side effect");
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
