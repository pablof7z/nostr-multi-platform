//! Compile-count smoke tests plus the `apply_selection` selection-budget
//! wiring: relay-cap pruning under `max_connections`, operator-pinned
//! app-relay preservation, dropped-relay CLOSE emission on the next
//! recompile, and `set_indexer_relays` threading into the compiler.
use super::*;
use crate::planner::{InMemoryMailboxCache, MailboxSnapshot};

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
        push_legacy(l.registry_mut(), follow(u64::from(i) + 1, &author_seed));
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
        push_legacy(l.registry_mut(), follow(u64::from(i) + 1, &author_seed));
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
        push_legacy(l.registry_mut(), follow(u64::from(i) + 1, &author_seed));
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
        &["wss://indexer-relay.example".to_string()],
        "cfg(test) default indexer set is a placeholder relay",
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
