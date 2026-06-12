//! Finding B — publishing to an AUTH-requiring relay must PARK until the
//! relay reaches NIP-42 `Authenticated`, never burn a fast retry budget and
//! falsely settle `FailedAfterRetries`.
//!
//! These engine-level tests assert the park semantics in isolation (no kernel,
//! no sockets): an `auth-required` ack demotes the relay to durable `Pending`
//! via the existing `unavailable_relays` + InFlight→Pending machinery (the same
//! availability gate connection-loss uses), and the parked publish dispatches
//! again only when `mark_relay_available` is called (the engine-side mirror of
//! the relay reaching `Authenticated`). The kernel-side wiring
//! (`RelayAuthState::Authenticated` → `mark_publish_relay_available`) is covered
//! by `kernel/publish_engine_tests.rs`.

use std::sync::Arc;

use super::PublishEngine;
use crate::publish::action::{PublishAction, PublishTarget};
use crate::publish::state::{PerRelayState, RelayAck, RetryPolicy};
use crate::publish::traits::{
    InMemoryPublishStore, NoopSigner, RelayDispatcher, ReplayDispatcher, StaticOutbox,
};
use crate::substrate::{SignedEvent, UnsignedEvent};

const AUTH_RELAY: &str = "wss://auth-relay.test";

fn signed_event(id: &str, author: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{id}"),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: format!("content-{id}"),
            created_at: 1_700_000_000,
        },
    }
}

fn engine_routed_to(relay: &str, dispatcher: Arc<ReplayDispatcher>) -> PublishEngine {
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert("alice".to_string(), vec![relay.to_string()]);
    PublishEngine::new(
        Arc::new(outbox),
        dispatcher as Arc<dyn RelayDispatcher>,
        Arc::new(InMemoryPublishStore::new()),
        Arc::new(NoopSigner),
        RetryPolicy::default(),
    )
}

#[test]
fn auth_required_ack_parks_relay_pending_does_not_settle_failed() {
    // A single-relay publish to an AUTH-requiring relay. The relay's first OK
    // frame is `auth-required` (it CLOSED/OK-false before we authenticated).
    // The publish must PARK: the per-relay state demotes back to durable
    // `Pending` (awaiting auth) — it must NOT settle `FailedAfterRetries`, and
    // a plain retry tick must NOT re-dispatch it (the relay is parked
    // unavailable until it authenticates).
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script(
        AUTH_RELAY,
        vec![RelayAck::failed(AUTH_RELAY, "auth-required", "auth-required: please AUTH")],
    );
    let mut engine = engine_routed_to(AUTH_RELAY, Arc::clone(&dispatcher));

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "park-1".to_string(),
                event: signed_event("ev-park-1", "alice"),
                target: PublishTarget::Auto,
            },
            100,
            None,
        )
        .unwrap();

    // Parked, not settled: the row is still in-flight (not evicted) and the
    // per-relay state is durable Pending.
    let per_relay = engine.per_relay(&"park-1".to_string());
    assert_eq!(
        per_relay.get(AUTH_RELAY),
        Some(&PerRelayState::Pending),
        "auth-required parks the relay back to durable Pending, not FailedAfterRetries: {per_relay:?}"
    );

    // The publish has NOT settled — no terminal outcome was recorded.
    assert!(
        engine.take_completed().is_empty(),
        "a parked publish must not settle (no terminal outcome)"
    );

    // A plain retry tick must NOT re-dispatch the parked relay (it stays
    // unavailable until auth). Only the original send frame was ever sent.
    engine.tick(200);
    assert_eq!(
        dispatcher.sent_frames().len(),
        1,
        "parked relay is not re-dispatched by a retry tick — only the original send frame exists"
    );
    assert_eq!(
        engine.per_relay(&"park-1".to_string()).get(AUTH_RELAY),
        Some(&PerRelayState::Pending),
        "still parked after a tick"
    );
}

#[test]
fn parked_publish_dispatches_and_succeeds_when_relay_becomes_available() {
    // After the relay authenticates (engine-side: `mark_relay_available`), the
    // parked publish re-dispatches and the relay's now-authenticated socket
    // accepts the EVENT (scripted OK), settling the publish to Ok.
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script(
        AUTH_RELAY,
        vec![
            RelayAck::failed(AUTH_RELAY, "auth-required", "auth-required: please AUTH"),
            RelayAck::ok(AUTH_RELAY),
        ],
    );
    let mut engine = engine_routed_to(AUTH_RELAY, Arc::clone(&dispatcher));

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "park-2".to_string(),
                event: signed_event("ev-park-2", "alice"),
                target: PublishTarget::Auto,
            },
            100,
            None,
        )
        .unwrap();

    // Parked.
    assert_eq!(
        engine.per_relay(&"park-2".to_string()).get(AUTH_RELAY),
        Some(&PerRelayState::Pending)
    );

    // Relay reaches `Authenticated` → engine-side availability callback.
    engine.mark_relay_available(AUTH_RELAY, 300).unwrap();

    // The publish re-dispatched (second scripted ack = OK) and settled Ok.
    let drained = engine.take_completed();
    assert_eq!(drained.len(), 1, "parked publish settles once after auth");
    let outcome = &drained[0];
    assert_eq!(outcome.event_id, "ev-park-2");
    assert_eq!(
        outcome.accepted,
        vec![AUTH_RELAY.to_string()],
        "the authenticated relay accepted the re-dispatched EVENT"
    );
    assert!(outcome.failed.is_empty(), "no failures: {outcome:?}");

    // The row was re-dispatched exactly once after the park (2 frames total:
    // original send + post-auth re-dispatch).
    assert_eq!(
        dispatcher.sent_frames().len(),
        2,
        "exactly one re-dispatch after auth — no budget-driven retries in between"
    );
}

#[test]
fn auth_round_trip_does_not_consume_transient_retry_budget() {
    // The park must not spend the transient retry budget: after the auth
    // round-trip the relay still has its FULL retry ladder if it later returns
    // a genuinely transient failure. Sequence: auth-required (park) → auth →
    // re-dispatch hits a transient "io" failure → that should still be
    // ScheduleRetry (attempt 1, retry budget intact), NOT a near-terminal
    // attempt count inflated by the auth round-trip.
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script(
        AUTH_RELAY,
        vec![
            RelayAck::failed(AUTH_RELAY, "auth-required", "auth-required"),
            RelayAck::failed(AUTH_RELAY, "io", "transient blip"),
        ],
    );
    let mut engine = engine_routed_to(AUTH_RELAY, Arc::clone(&dispatcher));

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "park-3".to_string(),
                event: signed_event("ev-park-3", "alice"),
                target: PublishTarget::Auto,
            },
            100,
            None,
        )
        .unwrap();

    // Park consumed no budget — re-dispatch on auth.
    engine.mark_relay_available(AUTH_RELAY, 300).unwrap();

    // The transient "io" on the re-dispatch is attempt 1 of the transient
    // ladder (RelayError + a scheduled retry), NOT FailedAfterRetries. If the
    // auth round-trip had consumed budget, this would already be near/at the
    // give-up threshold.
    let per_relay = engine.per_relay(&"park-3".to_string());
    match per_relay.get(AUTH_RELAY) {
        Some(PerRelayState::RelayError { attempt, .. }) => {
            assert_eq!(
                *attempt, 1,
                "transient failure after auth is attempt 1 — the auth park spent no transient budget"
            );
        }
        other => panic!(
            "expected RelayError(attempt=1) with full transient budget intact, got {other:?}"
        ),
    }
    assert!(
        engine.take_completed().is_empty(),
        "still retrying transiently — not terminally failed by a budget the auth park stole"
    );
}
