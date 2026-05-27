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
        is_indexer_discovery: false,
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
///
/// Note: this test deliberately does NOT call `set_app_relays`. Operator-
/// configured app relays carry the `UserConfigured(AppRelay)` lane and are
/// exempt from coverage pruning (operator-intent override; see
/// `selection.rs::relay_is_operator_pinned`). Including one here would
/// preserve it regardless of budget and obscure the actual selector test —
/// the carve-out's coverage lives in
/// `subs::lifecycle_tests::recompile_preserves_app_relay_under_budget`.
#[test]
fn recompile_caps_per_relay_at_max_connections() {
    let mut l = SubscriptionLifecycle::new();
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
        l.registry_mut()
            .push(follow(u64::from(i) + 1, &author_seed));
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

/// Companion to `recompile_caps_per_relay_at_max_connections`: when an
/// operator-configured app relay is added on top of the same 10-follow
/// scenario, the app relay MUST survive selection regardless of the
/// `max_connections` budget — and the budget still bounds the NIP-65
/// outbox relays alongside it. End state: 5 outbox relays + 1 app relay = 6.
///
/// This is the regression guard for the gallery-TUI smoke bug: under
/// `app_relays=[primal]` + an author with [atlas, eden] outbox, the
/// selector dropped primal because the outbox already covered the author
/// under `max_per_user=2`. Operator intent must override coverage.
#[test]
fn recompile_preserves_app_relay_under_budget() {
    let mut l = SubscriptionLifecycle::new();
    l.set_app_relays(vec!["wss://app.example".to_string()]);
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
        l.registry_mut()
            .push(follow(u64::from(i) + 1, &author_seed));
    }

    let _frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let plan = l.current_plan.as_ref().expect("plan present");

    assert!(
        plan.per_relay.contains_key("wss://app.example"),
        "operator-pinned app relay must survive selection regardless of \
         coverage budget; got per_relay keys: {:?}",
        plan.per_relay.keys().collect::<Vec<_>>(),
    );

    // The greedy budget still bounds the NIP-65 outbox relays alongside
    // the pinned app relay — total = pinned + at most max_connections.
    let outbox_count = plan
        .per_relay
        .keys()
        .filter(|k| k.as_str() != "wss://app.example")
        .count();
    assert!(
        outbox_count <= max_connections,
        "outbox-relay count = {} must remain ≤ max_connections = {} (the \
         pinned app relay must NOT consume the greedy budget); got: {:?}",
        outbox_count,
        max_connections,
        plan.per_relay.keys().collect::<Vec<_>>(),
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
        l.registry_mut()
            .push(follow(u64::from(i) + 1, &author_seed));
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
    let surviving: std::collections::BTreeSet<String> = plan.per_relay.keys().cloned().collect();

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
    assert_eq!(
        expected_dropped.len(),
        2,
        "two relays must have been dropped"
    );
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

    let _ = l.mark_relay_dead("wss://only.example".to_string());
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
    l.registry_mut().push(follow(1, "aa"));

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

// ─── lifecycle.rs unit tests — constructor + accessors/setters ──────────
//
// These pin the surface defined in `subs/lifecycle.rs` (the `new`/`Default`
// impls, the dead-relay state machine, and the field accessors/setters).
// `lifecycle_tests` is a child module of `subs`, so it may read the private
// `inbox`/`probed_mailboxes` fields of `SubscriptionLifecycle` to assert the
// exact triggers each transition enqueues without routing through a compile.
// `CompileTrigger` derives only `Clone, Debug` (no `PartialEq`), so trigger
// payloads are pattern-matched rather than compared with `assert_eq!`.

/// `new()` and `Default::default()` must both yield the same empty zero-state:
/// no compiles, no plan, no dead relays, no probed mailboxes, no planner
/// error, and an empty trigger inbox. The `#[cfg(test)]` build seeds
/// `indexer_relays` with the purplepag.es default — assert that too so a
/// regression in either constructor surfaces here.
#[test]
fn new_and_default_produce_identical_empty_state() {
    let from_new = SubscriptionLifecycle::new();
    let from_default = SubscriptionLifecycle::default();

    for (label, l) in [("new", &from_new), ("default", &from_default)] {
        assert_eq!(
            l.compile_count(),
            0,
            "{label}: compile_count must start at 0"
        );
        assert!(l.current_plan.is_none(), "{label}: no plan at construction");
        assert!(l.dead_relays().is_empty(), "{label}: dead_relays empty");
        assert!(l.probed_mailboxes().is_empty(), "{label}: probed set empty");
        assert!(
            l.last_planner_error().is_none(),
            "{label}: no planner error at construction",
        );
        assert!(l.inbox.is_empty(), "{label}: trigger inbox empty");
        assert_eq!(
            l.indexer_relays(),
            ["wss://purplepag.es".to_string()].as_slice(),
            "{label}: cfg(test) indexer default must be purplepag.es",
        );
    }
}

/// The first `mark_relay_dead` for a URL changes state: it returns `true`,
/// inserts the URL into `dead_relays`, and enqueues exactly one
/// `RelayHealthChanged { dead: true }` carrying that URL.
#[test]
fn mark_relay_dead_first_call_inserts_and_enqueues_trigger() {
    let mut l = SubscriptionLifecycle::new();
    let url = "wss://dead.example".to_string();

    let changed = l.mark_relay_dead(url.clone());

    assert!(changed, "first mark_relay_dead must report a state change");
    assert!(l.dead_relays().contains(&url), "URL must be in dead_relays");

    let drained = l.inbox.drain_coalesced();
    assert_eq!(drained.len(), 1, "exactly one trigger must be enqueued");
    match &drained[0] {
        CompileTrigger::RelayHealthChanged { url: u, dead } => {
            assert_eq!(u, &url, "trigger must carry the marked URL");
            assert!(*dead, "trigger must report dead = true");
        }
        other => panic!("expected RelayHealthChanged, got {other:?}"),
    }
}

/// `mark_relay_dead` is idempotent: marking an already-dead relay dead again
/// returns `false` and enqueues NO further trigger (the inbox stays at the
/// single trigger from the original transition).
#[test]
fn mark_relay_dead_second_call_is_noop_and_enqueues_nothing() {
    let mut l = SubscriptionLifecycle::new();
    let url = "wss://dead.example".to_string();

    assert!(l.mark_relay_dead(url.clone()), "first call changes state");
    let after_first = l.inbox.len();

    let changed_again = l.mark_relay_dead(url.clone());

    assert!(
        !changed_again,
        "re-marking a dead relay must report no change"
    );
    assert_eq!(
        l.inbox.len(),
        after_first,
        "no redundant trigger may be enqueued for an unchanged relay",
    );
    assert_eq!(l.dead_relays().len(), 1, "dead_relays must not grow");
}

/// Clearing a previously-dead relay returns `true`, removes it from
/// `dead_relays`, and enqueues a symmetric `RelayHealthChanged { dead: false }`
/// so affected authors can route back onto the relay on the next compile.
#[test]
fn mark_relay_alive_clears_dead_relay_and_enqueues_recovery_trigger() {
    let mut l = SubscriptionLifecycle::new();
    let url = "wss://flaky.example".to_string();

    let _ = l.mark_relay_dead(url.clone());
    // Discard the dead-trigger so we observe only the recovery trigger.
    let _ = l.inbox.drain_coalesced();

    let recovered = l.mark_relay_alive(&url);

    assert!(recovered, "marking a dead relay alive must report a change");
    assert!(
        !l.dead_relays().contains(&url),
        "recovered relay must leave dead_relays",
    );

    let drained = l.inbox.drain_coalesced();
    assert_eq!(drained.len(), 1, "exactly one recovery trigger expected");
    match &drained[0] {
        CompileTrigger::RelayHealthChanged { url: u, dead } => {
            assert_eq!(u, &url, "recovery trigger must carry the recovered URL");
            assert!(!*dead, "recovery trigger must report dead = false");
        }
        other => panic!("expected RelayHealthChanged, got {other:?}"),
    }
}

/// `mark_relay_alive` on a relay that was never dead is a pure no-op: it
/// returns `false` and enqueues no trigger.
#[test]
fn mark_relay_alive_on_never_dead_relay_is_noop() {
    let mut l = SubscriptionLifecycle::new();

    let changed = l.mark_relay_alive(&"wss://healthy.example".to_string());

    assert!(
        !changed,
        "clearing a never-dead relay must report no change"
    );
    assert!(l.inbox.is_empty(), "no trigger may be enqueued for a no-op");
}

// ─── PD-033-C — `set_bootstrap_content_relays` end-to-end wiring ─────────────

/// PD-033-C planner extension end-to-end smoke: setting bootstrap content
/// relays on the lifecycle and registering a `OneShot + Global + event_ids`
/// discovery interest produces a `WireFrame::Req` addressed to the bootstrap
/// URL. Proves the lifecycle threads the new field into the compiler and the
/// compiler's new gate fires through to the wire-emitter.
///
/// This is the integration counterpart to the in-tree
/// `case_d_no_author::tests::pd033c_event_ids_oneshot_global_routes_to_bootstrap_content`
/// unit test — together they pin the planner-side prerequisite for Stage 1.
#[test]
fn pd033c_bootstrap_content_relays_threaded_into_recompile() {
    let mut l = SubscriptionLifecycle::new();
    // Drop the cfg(test) purplepag.es default so we can prove the discovery
    // REQ lands on bootstrap content, not the indexer fallback.
    l.set_indexer_relays(vec![]);
    l.set_bootstrap_content_relays(vec!["wss://relay.primal.net".to_string()]);

    let mailboxes = InMemoryMailboxCache::new();
    // Discovery oneshot for an event id — matches `oneshot.request(...)` in
    // `kernel/discovery.rs::drain_unknown_oneshots` (events arm).
    let event_id_hex: String = "aa".repeat(32);
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            event_ids: [event_id_hex.clone()].into_iter().collect(),
            limit: Some(1),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    };
    l.registry_mut().push(interest);

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let landed: Vec<&WireFrame> = frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url, .. } if relay_url == "wss://relay.primal.net"))
        .collect();
    assert_eq!(
        landed.len(),
        1,
        "exactly one REQ must land on the bootstrap content relay; got {} frames in total: {:?}",
        landed.len(),
        frames
    );
    if let WireFrame::Req {
        lifecycle,
        filter_json,
        ..
    } = landed[0]
    {
        assert!(
            matches!(lifecycle, InterestLifecycle::OneShot),
            "the bootstrap REQ must carry OneShot lifecycle (CLOSE on EOSE)"
        );
        assert!(
            filter_json.contains(&event_id_hex),
            "the bootstrap REQ filter must carry the discovery event_id; got {filter_json}"
        );
    } else {
        panic!("expected a WireFrame::Req on the bootstrap relay");
    }
}

/// `set_bootstrap_content_relays` REPLACES the bootstrap set wholesale —
/// matches the `set_indexer_relays` / `set_app_relays` setter contract. An
/// empty Vec disables the bootstrap gate, falling back to the unchanged
/// Case D body.
#[test]
fn pd033c_set_bootstrap_content_relays_replaces_rather_than_appends() {
    let mut l = SubscriptionLifecycle::new();
    l.set_indexer_relays(vec![]);
    l.set_bootstrap_content_relays(vec!["wss://first.example".to_string()]);
    l.set_bootstrap_content_relays(vec![
        "wss://second.example".to_string(),
        "wss://third.example".to_string(),
    ]);

    let mailboxes = InMemoryMailboxCache::new();
    let event_id_hex: String = "bb".repeat(32);
    l.registry_mut().push(LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            event_ids: [event_id_hex].into_iter().collect(),
            limit: Some(1),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    });

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let urls: std::collections::BTreeSet<String> = frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();
    assert!(
        urls.contains("wss://second.example") && urls.contains("wss://third.example"),
        "later setter call must REPLACE the prior set; got {urls:?}"
    );
    assert!(
        !urls.contains("wss://first.example"),
        "the first bootstrap URL must have been replaced, not retained; got {urls:?}"
    );
}

/// PD-033-C end-to-end smoke for the profile-oneshot arm: setting
/// `bootstrap_indexer_relays` on the lifecycle and registering a `OneShot +
/// Global + authors`-shaped profile-fetch interest (no NIP-65 mailbox, no
/// app_relays) produces a `WireFrame::Req` addressed to the bootstrap indexer.
/// Mirrors `kernel/discovery.rs::drain_unknown_oneshots`'s profile-oneshot
/// fan-out — the planner-side parity check Stage 1 depends on.
#[test]
fn pd033c_bootstrap_indexer_relays_threaded_into_recompile() {
    let mut l = SubscriptionLifecycle::new();
    // Drop the cfg(test) raw indexer default so we can prove the discovery
    // REQ lands on the BOOTSTRAP indexer specifically (not the raw one — the
    // cold-start divergence the planner extension fixes).
    l.set_indexer_relays(vec![]);
    l.set_bootstrap_indexer_relays(vec!["wss://purplepag.es".to_string()]);

    let mailboxes = InMemoryMailboxCache::new();
    // Profile-shape oneshot — matches `oneshot.request(...)` in
    // `kernel/discovery.rs::drain_unknown_oneshots` (profiles arm).
    let bob: String = "ab".repeat(32);
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [bob.clone()].into_iter().collect(),
            kinds: [0u32, 3, 10002].into_iter().collect(),
            limit: Some(3),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        // The planner-extension bootstrap-indexer fallback gate is now the
        // explicit `is_indexer_discovery` flag (was: OneShot + Global). The
        // discovery-direction profile-shape interest opts in.
        is_indexer_discovery: true,
    };
    l.registry_mut().push(interest);

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let landed: Vec<&WireFrame> = frames
        .iter()
        .filter(
            |f| matches!(f, WireFrame::Req { relay_url, .. } if relay_url == "wss://purplepag.es"),
        )
        .filter(|f| match f {
            // Discriminate the bootstrap-indexer profile fetch from any
            // mailbox-probe REQ that might also land on the same URL — the
            // probe is a separate auxiliary frame and its sub_id is prefixed.
            WireFrame::Req { sub_id, .. } => !sub_id.starts_with("mailbox-probe-"),
            _ => false,
        })
        .collect();
    assert_eq!(
        landed.len(),
        1,
        "exactly one profile-fetch REQ must land on the bootstrap indexer; \
         got {} matching frames in {} total",
        landed.len(),
        frames.len(),
    );
    if let WireFrame::Req {
        lifecycle,
        filter_json,
        ..
    } = landed[0]
    {
        assert!(
            matches!(lifecycle, InterestLifecycle::OneShot),
            "the bootstrap-indexer REQ must carry OneShot lifecycle"
        );
        assert!(
            filter_json.contains(&bob),
            "the bootstrap-indexer REQ filter must carry the discovery author; got {filter_json}"
        );
    } else {
        panic!("expected a WireFrame::Req on the bootstrap indexer");
    }
    // PD-033-C invariant: the discovery author MUST NOT be unroutable.
    assert!(
        !l.current_plan_unroutable().contains(&bob),
        "PD-033-C invariant: the discovery-oneshot author must NOT be unroutable"
    );
}

/// `set_indexer_relays` REPLACES the indexer set wholesale — it does not
/// append to the `#[cfg(test)]` purplepag.es default. Setting an empty Vec
/// disables the indexer fallback entirely.
#[test]
fn set_indexer_relays_replaces_rather_than_appends() {
    let mut l = SubscriptionLifecycle::new();
    // cfg(test) default is the single purplepag.es entry.
    assert_eq!(l.indexer_relays().len(), 1);

    l.set_indexer_relays(vec![
        "wss://relay.one".to_string(),
        "wss://relay.two".to_string(),
    ]);
    assert_eq!(
        l.indexer_relays(),
        ["wss://relay.one".to_string(), "wss://relay.two".to_string()].as_slice(),
        "set_indexer_relays must replace the default, not append to it",
    );

    l.set_indexer_relays(Vec::new());
    assert!(
        l.indexer_relays().is_empty(),
        "an empty Vec must disable the indexer fallback",
    );
}

/// `last_planner_error` round-trips through the `#[cfg(test)]`
/// `set_planner_error_for_test` seam: `None` at construction, then the
/// injected string, with latest-error-wins semantics on a second injection.
#[test]
fn last_planner_error_round_trips_through_test_seam() {
    let mut l = SubscriptionLifecycle::new();
    assert!(l.last_planner_error().is_none(), "no error at construction");

    l.set_planner_error_for_test("InvalidShape: empty kind set");
    assert_eq!(
        l.last_planner_error(),
        Some("InvalidShape: empty kind set"),
        "injected error must be observable",
    );

    l.set_planner_error_for_test("HashingFailed");
    assert_eq!(
        l.last_planner_error(),
        Some("HashingFailed"),
        "latest-error-wins: the second injection must overwrite the first",
    );
}

/// `clear_probed_mailboxes` empties the implicit-discovery probed set — the
/// `refresh` escape hatch that forces every still-unknown author to be
/// re-probed on the next recompile. The set is seeded directly via the
/// private field (no public setter exists; descendant-module access applies).
#[test]
fn clear_probed_mailboxes_empties_the_probed_set() {
    let mut l = SubscriptionLifecycle::new();
    l.probed_mailboxes.insert(pubkey("a"));
    l.probed_mailboxes.insert(pubkey("b"));
    assert_eq!(l.probed_mailboxes().len(), 2, "probed set seeded with 2");

    l.clear_probed_mailboxes();

    assert!(
        l.probed_mailboxes().is_empty(),
        "clear_probed_mailboxes must empty the set so authors are re-probed",
    );
}
