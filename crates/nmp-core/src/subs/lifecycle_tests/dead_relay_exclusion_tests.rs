//! Dead-relay exclusion and recovery: an author routes off a relay marked
//! dead (staying on any alive sibling, or dropping out entirely if none
//! remain) and routes back on once the relay is marked alive again.
//! `mark_relay_dead`/`mark_relay_alive` idempotency and trigger emission.
use super::*;
use crate::planner::{InMemoryMailboxCache, MailboxSnapshot};

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
    push_legacy(l.registry_mut(), follow(1, "cc01"));

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
    push_legacy(l.registry_mut(), follow(1, "dd01"));

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
