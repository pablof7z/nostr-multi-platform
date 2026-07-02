//! `lifecycle.rs` unit tests — constructor + accessors/setters.
//!
//! These pin the surface defined in `subs/lifecycle.rs` (the `new`/`Default`
//! impls, the dead-relay state machine, and the field accessors/setters).
//! This module is a descendant of `subs` (via `lifecycle_tests`), so it may
//! read the private `inbox`/`probed_mailboxes` fields of
//! `SubscriptionLifecycle` to assert the exact triggers each transition
//! enqueues without routing through a compile. `CompileTrigger` derives only
//! `Clone, Debug` (no `PartialEq`), so trigger payloads are pattern-matched
//! rather than compared with `assert_eq!`.
use super::*;

/// `new()` and `Default::default()` must both yield the same empty zero-state:
/// no compiles, no plan, no dead relays, no probed mailboxes, no planner
/// error, and an empty trigger inbox. The `#[cfg(test)]` build seeds
/// `indexer_relays` with the cfg(test) placeholder default — assert that too so a
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
            ["wss://indexer-relay.example".to_string()].as_slice(),
            "{label}: cfg(test) indexer default must remain the placeholder",
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
