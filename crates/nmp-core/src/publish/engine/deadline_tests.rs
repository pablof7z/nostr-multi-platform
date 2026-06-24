//! Publish-engine deadline predicate tests for the wasm runtime scheduler.

use std::sync::Arc;

use super::{PublishEngine, PublishQueueTerminal};
use crate::publish::action::{PublishAction, PublishTarget};
use crate::publish::state::RetryPolicy;
use crate::publish::traits::{
    InMemoryPublishStore, NoopSigner, QueueDispatcher, RelayDispatcher, StaticOutbox,
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
        Arc::new(NoopSigner),
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
