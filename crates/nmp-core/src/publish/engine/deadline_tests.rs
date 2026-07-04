//! Publish-engine deadline predicate tests for the wasm runtime scheduler.

use std::sync::Arc;

use super::{PublishEngine, PublishQueueTerminal};
use crate::publish::action::{PublishAction, PublishTarget};
use crate::publish::state::{RelayAck, RetryPolicy};
use crate::publish::traits::{
    InMemoryPublishStore, QueueDispatcher, RelayDispatcher, StaticOutbox,
};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

fn signed_event(id: &str, author: &str, kind: u32) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{id}"),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: Vec::new(),
            content: format!("content-{id}"),
            created_at: 1_700_000_000,
        },
    }
}

#[test]
fn next_deadline_reports_inflight_timeout_and_due_tick_settles() {
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert("alice".to_string(), vec!["wss://silent".to_string()]);
    let dispatcher = Arc::new(QueueDispatcher::new());
    let policy = RetryPolicy {
        transient_max_retries: 1,
        inflight_deadline_ms: 5_000,
        ..RetryPolicy::default()
    };
    let mut engine = PublishEngine::new(
        Arc::new(outbox),
        dispatcher as Arc<dyn RelayDispatcher>,
        Arc::new(InMemoryPublishStore::new()),
        policy,
    );

    let t0 = 1_000_000;
    engine
        .start_publish(
            PublishAction::Publish {
                handle: "deadline-handle".to_string(),
                event: signed_event("deadline-event", "alice", 1),
                target: PublishTarget::Auto,
            },
            t0,
            None,
        )
        .unwrap();

    assert_eq!(
        engine.next_deadline_ms(t0),
        Some(t0 + 5_000),
        "in-flight publish must declare its timeout deadline"
    );

    engine.tick(t0 + 4_999);
    assert!(
        engine.take_pending_terminals().is_empty(),
        "work must not fire before its declared deadline"
    );
    assert_eq!(engine.next_deadline_ms(t0 + 4_999), Some(t0 + 5_000));

    engine.tick(t0 + 5_000);
    let completed: Vec<_> = engine
        .take_pending_terminals()
        .into_iter()
        .filter_map(|terminal| match terminal.publish_queue {
            PublishQueueTerminal::Settled(outcome) => Some(outcome),
            PublishQueueTerminal::Cancelled { .. } | PublishQueueTerminal::None => None,
        })
        .collect();
    assert_eq!(completed.len(), 1, "due timed work must fire on tick");
    assert!(
        completed[0]
            .failed
            .iter()
            .any(|(url, _)| url == "wss://silent"),
        "the silent relay must settle failed after the due timeout"
    );
    assert_eq!(
        engine.next_deadline_ms(t0 + 5_000),
        None,
        "settled work must not leave a deadline behind"
    );
}

/// #2967 regression: a relay that goes unavailable (the kernel's
/// `PoolEvent::Failed`/`Closed` path — socket dial failed, mid-session drop)
/// and NEVER reconnects must not park the whole publish handle `Pending`
/// forever. `dispatch_due`/`next_deadline_ms` deliberately skip relays in
/// `unavailable_relays` (no point re-dialing a socket already known to be
/// down), so before the #2967 fix nothing ever aged that row out — a single
/// permanently-dead relay in a multi-relay publish set blocked `is_complete`
/// indefinitely even after every OTHER relay already accepted the event.
///
/// This test proves the OK-from-any / fail-fast bound the outbox model
/// implies: the good relay's acceptance is never revisited, and the whole
/// handle settles `Mixed` once `policy.inflight_deadline_ms` has elapsed
/// since the dead relay was marked unavailable — bounded, not indefinite.
#[test]
fn permanently_unavailable_relay_fails_fast_without_blocking_the_good_relay() {
    let mut outbox = StaticOutbox::default();
    outbox.author_writes.insert(
        "alice".to_string(),
        vec![
            "wss://dead-relay".to_string(),
            "wss://good-relay".to_string(),
        ],
    );
    let dispatcher = Arc::new(QueueDispatcher::new());
    let policy = RetryPolicy {
        transient_max_retries: 1,
        inflight_deadline_ms: 5_000,
        ..RetryPolicy::default()
    };
    let mut engine = PublishEngine::new(
        Arc::new(outbox),
        dispatcher as Arc<dyn RelayDispatcher>,
        Arc::new(InMemoryPublishStore::new()),
        policy,
    );

    let handle = "dead-relay-handle".to_string();
    let t0 = 1_000_000;
    engine
        .start_publish(
            PublishAction::Publish {
                handle: handle.clone(),
                event: signed_event("dead-relay-event", "alice", 1),
                target: PublishTarget::Auto,
            },
            t0,
            None,
        )
        .unwrap();

    // The good relay acks OK almost immediately — this must never be
    // revisited or held hostage by the dead relay's fate.
    engine.on_ack(&handle, RelayAck::ok("wss://good-relay"), t0 + 50);
    assert!(
        engine.take_pending_terminals().is_empty(),
        "the publish is not complete yet — the dead relay is still pending"
    );

    // The kernel observes the dead relay's socket dial fail (`PoolEvent::Failed`)
    // shortly after and marks it unavailable. It never reconnects — no
    // `mark_relay_available` call ever follows, simulating a genuinely dead
    // relay (e.g. `wss://relay.nostr.band` timing out from the reporter's
    // network in #2967).
    let unavailable_at = t0 + 100;
    engine
        .mark_relay_unavailable("wss://dead-relay", unavailable_at)
        .unwrap();

    // Well before the deadline: still not complete (bounded, not instant).
    engine.tick(unavailable_at + policy.inflight_deadline_ms - 1);
    assert!(
        engine.take_pending_terminals().is_empty(),
        "must not fail fast before its declared deadline — this proves the \
         bound is real, not a no-op that always settles immediately"
    );

    // At the deadline: the dead relay is force-settled and the handle
    // completes with a Mixed outcome — accepted by the good relay, failed on
    // the dead one. No indefinite hang.
    engine.tick(unavailable_at + policy.inflight_deadline_ms);
    let completed: Vec<_> = engine
        .take_pending_terminals()
        .into_iter()
        .filter_map(|terminal| match terminal.publish_queue {
            PublishQueueTerminal::Settled(outcome) => Some(outcome),
            PublishQueueTerminal::Cancelled { .. } | PublishQueueTerminal::None => None,
        })
        .collect();
    assert_eq!(
        completed.len(),
        1,
        "the publish must settle once the dead relay's deadline elapses, \
         not hang indefinitely (#2967)"
    );
    assert_eq!(
        completed[0].accepted,
        vec!["wss://good-relay".to_string()],
        "the reachable relay's earlier OK must be preserved in the final outcome"
    );
    assert!(
        completed[0]
            .failed
            .iter()
            .any(|(url, _)| url == "wss://dead-relay"),
        "the permanently-unavailable relay must settle failed: {:?}",
        completed[0].failed
    );
}
