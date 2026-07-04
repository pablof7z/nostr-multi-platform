//! Offline-first publish intent regression tests.
//!
//! These cover the publish engine directly so relay availability is explicit:
//! an unavailable relay keeps its publish row durable and `Pending`, while
//! reconnect/retry paths release that same intent without local ingest shims.

use std::sync::{Arc, Mutex};

use nmp_core::publish::{
    InMemoryPublishStore, OutboxResolver, PerRelayState, PublishAction, PublishEngine,
    PublishRouteClass, PublishStore, PublishTarget, QueueDispatcher, RelayAck, RelayDispatcher,
    RelaySelectionReason, ResolvedRelay, RetryPolicy, StaticOutbox,
};
use nmp_core::substrate::BlockedRelaySet;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

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
                target: PublishTarget::explicit(
                    vec![relay.to_string()],
                    PublishRouteClass::ManualOverride,
                ),
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
                target: PublishTarget::explicit(
                    vec![relay.to_string()],
                    PublishRouteClass::ManualOverride,
                ),
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

/// Resolver whose relay set can change mid-test — simulates a user adding a
/// relay to their configured relay set after a note was already queued.
/// `StaticOutbox`/`ReplayDispatcher`'s fixed test doubles elsewhere in this
/// crate all snapshot their relay set once at construction, which cannot
/// model "the outbox resolver's answer changes after the row was resolved" —
/// exactly the scenario NMP#3020 was blind to.
struct DynamicResolver {
    relays: Mutex<Vec<String>>,
}

impl DynamicResolver {
    fn new(initial: &[&str]) -> Self {
        Self {
            relays: Mutex::new(initial.iter().map(|s| (*s).to_string()).collect()),
        }
    }

    /// Simulate the user adding `url` to their configured relay set.
    fn add_relay(&self, url: &str) {
        self.relays.lock().unwrap().push(url.to_string());
    }
}

impl OutboxResolver for DynamicResolver {
    fn resolve(
        &self,
        _author_pubkey: &str,
        _p_tags: &[String],
        _target: &PublishTarget,
        _kind: u32,
        blocked: &BlockedRelaySet,
    ) -> Vec<ResolvedRelay> {
        self.relays
            .lock()
            .unwrap()
            .iter()
            .filter(|url| !blocked.contains(url))
            .map(|url| ResolvedRelay {
                url: url.clone(),
                reason: RelaySelectionReason::AuthorWriteRelay,
            })
            .collect()
    }
}

/// NMP#3020 regression: before this fix, `sweep_unavailable_timeouts` force-
/// settled ANY row whose sole target relay had been continuously unavailable
/// for `policy.inflight_deadline_ms` — even when NO relay had ever accepted
/// the event — finalizing it `FailedAfterRetries` and deleting it from
/// `in_flight`/the durable store via `finalize_completed_rows`. A note
/// composed while its only configured relay was dead was silently and
/// permanently lost 30s later, with no trace in the Outbox. This proves the
/// row instead stays durably `Pending` past the deadline, and that
/// `next_deadline_ms` stops reporting a deadline for it (no busy re-evaluation
/// — D8).
#[test]
fn single_dead_relay_publish_survives_the_retry_deadline_instead_of_being_evicted() {
    let relay = "wss://only-relay.test";
    let dispatcher = Arc::new(QueueDispatcher::new());
    let store = Arc::new(InMemoryPublishStore::new());
    let policy = RetryPolicy {
        inflight_deadline_ms: 5_000,
        ..RetryPolicy::default()
    };
    let mut engine = PublishEngine::new(
        Arc::new(StaticOutbox::default()),
        dispatcher.clone() as Arc<dyn RelayDispatcher>,
        store.clone(),
        policy,
    );
    engine.mark_relay_unavailable(relay, 0).unwrap();

    let handle = "single-dead-relay-h".to_string();
    engine
        .start_publish(
            PublishAction::Publish {
                handle: handle.clone(),
                event: signed("ev-single-dead", "alice", 1),
                target: PublishTarget::explicit(
                    vec![relay.to_string()],
                    PublishRouteClass::ManualOverride,
                ),
            },
            0,
            None,
        )
        .unwrap();

    // Tick well past the deadline that used to force-settle (and evict) this
    // row even though nothing had ever accepted it.
    let far_past_deadline = policy.inflight_deadline_ms * 10;
    engine.tick(far_past_deadline);

    assert_eq!(
        engine.per_relay(&handle).get(relay),
        Some(&PerRelayState::Pending),
        "the sole target relay must stay durably Pending, not be force-settled"
    );
    let pending = store.load_pending().unwrap();
    assert_eq!(
        pending.len(),
        1,
        "the row must still be present in the durable store — never silently evicted"
    );
    assert!(
        engine.next_deadline_ms(far_past_deadline).is_none(),
        "with no accepted relay the engine must not keep reporting a deadline that will \
         never actually fire (no busy re-evaluation)"
    );

    // The relay finally reconnects — the still-pending intent is delivered.
    engine
        .mark_relay_available(relay, far_past_deadline + 50)
        .unwrap();
    let frames = dispatcher.drain();
    assert_eq!(frames.len(), 1, "reconnect must flush the surviving intent");
    assert_eq!(frames[0].0, relay);
}

/// NMP#3020 end-to-end repro: compose while the only configured relay is
/// dead, cross the retry deadline, THEN add a brand-new relay to the
/// configured set and connect it. The note must be re-targeted onto the new
/// relay and delivered — this is the exact "compose-offline → reconnect →
/// flush" path the issue reported as permanently broken.
#[test]
fn relay_added_after_compose_and_deadline_retargets_and_delivers_to_it() {
    let dead_relay = "wss://dead-at-compose.test";
    let new_relay = "wss://added-after-compose.test";
    let resolver = Arc::new(DynamicResolver::new(&[dead_relay]));
    let dispatcher = Arc::new(QueueDispatcher::new());
    let store = Arc::new(InMemoryPublishStore::new());
    let policy = RetryPolicy {
        inflight_deadline_ms: 5_000,
        ..RetryPolicy::default()
    };
    let mut engine = PublishEngine::new(
        resolver.clone() as Arc<dyn OutboxResolver>,
        dispatcher.clone() as Arc<dyn RelayDispatcher>,
        store.clone(),
        policy,
    );
    engine.mark_relay_unavailable(dead_relay, 0).unwrap();

    let handle = "retarget-h".to_string();
    engine
        .start_publish(
            PublishAction::Publish {
                handle: handle.clone(),
                event: signed("ev-retarget", "alice", 1),
                target: PublishTarget::Auto,
            },
            0,
            None,
        )
        .unwrap();
    assert!(
        dispatcher.drain().is_empty(),
        "the dead relay must never be dispatched to"
    );
    assert_eq!(
        engine.per_relay(&handle).len(),
        1,
        "only the originally-resolved relay is tracked at compose time"
    );

    // Cross the retry deadline — before the #3020 fix this alone would have
    // force-settled and evicted the row.
    let far_past_deadline = policy.inflight_deadline_ms * 10;
    engine.tick(far_past_deadline);
    assert_eq!(
        store.load_pending().unwrap().len(),
        1,
        "row must survive the deadline with zero relays ever having accepted"
    );

    // The user adds a second, healthy relay to their configured relay set —
    // it was never a key in this row's `per_relay` map.
    resolver.add_relay(new_relay);
    assert!(
        !engine.per_relay(&handle).contains_key(new_relay),
        "the new relay must not appear until the engine actually retargets"
    );

    // The new relay connects.
    engine
        .mark_relay_available(new_relay, far_past_deadline + 100)
        .unwrap();

    assert!(
        engine.per_relay(&handle).contains_key(new_relay),
        "mark_relay_available must retarget the still-pending row onto the newly-added relay"
    );
    let frames = dispatcher.drain();
    assert_eq!(
        frames.len(),
        1,
        "the retargeted relay must receive the queued frame: {frames:?}"
    );
    assert_eq!(frames[0].0, new_relay);
    assert!(frames[0].1.contains("\"EVENT\""));

    // The original dead relay's target is untouched by the retarget — it
    // stays a distinct, still-pending row entry.
    assert_eq!(
        engine.per_relay(&handle).get(dead_relay),
        Some(&PerRelayState::Pending)
    );
}
