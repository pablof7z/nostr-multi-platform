//! Offline-first publish intent regression tests.
//!
//! These cover the publish engine directly so relay availability is explicit:
//! an unavailable relay keeps its publish row durable and `Pending`, while
//! reconnect/retry paths release that same intent without local ingest shims.

use std::sync::Arc;

use nmp_core::publish::{
    InMemoryPublishStore, NoopSigner, PerRelayState, PublishAction, PublishEngine, PublishStore,
    PublishTarget, QueueDispatcher, RelayAck, RelayDispatcher, RetryPolicy, StaticOutbox,
};
use nmp_core::substrate::{SignedEvent, UnsignedEvent};

fn signed(id: &str, author: &str, kind: u32) -> SignedEvent {
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

fn queue_engine(
    dispatcher: Arc<QueueDispatcher>,
    store: Arc<InMemoryPublishStore>,
) -> PublishEngine {
    PublishEngine::new(
        Arc::new(StaticOutbox::default()),
        dispatcher as Arc<dyn RelayDispatcher>,
        store,
        Arc::new(NoopSigner),
        RetryPolicy::default(),
    )
}

#[test]
fn offline_relay_keeps_publish_intent_pending_until_available() {
    let relay = "wss://offline-write.test";
    let dispatcher = Arc::new(QueueDispatcher::new());
    let store = Arc::new(InMemoryPublishStore::new());
    let mut engine = queue_engine(dispatcher.clone(), store.clone());
    engine.mark_relay_unavailable(relay, 0).unwrap();

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "offline-h".to_string(),
                event: signed("ev-offline", "alice", 1),
                target: PublishTarget::Explicit {
                    relays: vec![relay.to_string()],
                },
            },
            100,
            None,
        )
        .unwrap();

    assert!(dispatcher.drain().is_empty());
    assert_eq!(
        engine.per_relay(&"offline-h".to_string()).get(relay),
        Some(&PerRelayState::Pending)
    );
    let pending = store.load_pending().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(
        pending[0]
            .per_relay
            .iter()
            .any(|(url, state)| url == relay && state == &PerRelayState::Pending),
        "durable row keeps the offline target pending: {:?}",
        pending[0].per_relay
    );

    engine.mark_relay_available(relay, 200).unwrap();
    let frames = dispatcher.drain();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, relay);
    assert!(frames[0].1.contains("\"EVENT\""));
}

#[test]
fn retry_tick_dispatches_due_intent_after_relay_becomes_available() {
    let relay = "wss://retry-write.test";
    let dispatcher = Arc::new(QueueDispatcher::new());
    let store = Arc::new(InMemoryPublishStore::new());
    let mut engine = queue_engine(dispatcher.clone(), store);
    let handle = "retry-h".to_string();

    engine
        .start_publish(
            PublishAction::Publish {
                handle: handle.clone(),
                event: signed("ev-retry", "alice", 1),
                target: PublishTarget::Explicit {
                    relays: vec![relay.to_string()],
                },
            },
            0,
            None,
        )
        .unwrap();
    assert_eq!(dispatcher.drain().len(), 1);

    engine.on_ack(
        &handle,
        RelayAck::failed(relay, "io", "connection reset"),
        100,
    );
    engine.mark_relay_unavailable(relay, 200).unwrap();
    engine.tick(1_500);
    assert!(dispatcher.drain().is_empty());

    engine.mark_relay_available(relay, 500).unwrap();
    assert!(dispatcher.drain().is_empty());

    engine.tick(1_500);
    let frames = dispatcher.drain();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, relay);
}
