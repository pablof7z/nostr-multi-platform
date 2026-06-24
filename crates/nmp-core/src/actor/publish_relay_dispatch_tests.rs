//! Publish-output relay dispatch regression coverage.
//!
//! These tests stay at the actor relay boundary: the publish engine/commands
//! produce `OutboundMessage`s with concrete relay URLs, and relay lifecycle code
//! must either spawn a pool worker immediately or retain publish frames until
//! the actor is running again.
//!
//! Phase F: post-cut-over the actor's per-URL transport is a
//! [`nmp_network::pool::Pool`]; these tests construct a fresh pool the same
//! way the actor runtime does and assert the bookkeeping invariants survive.

use super::commands::{
    create_account, new_bunker_handshake_slot, publish_signed_event, IdentityRuntime,
};
use super::relay_mgmt::{close_relays, route_dispatch_outbound};
use super::relay_runtime::RelayRuntime;
use crate::kernel::Kernel;
use crate::publish::PublishTarget;
use crate::relay::{CanonicalRelayUrl, OutboundMessage, DEFAULT_VISIBLE_LIMIT};
use nmp_network::pool::{Pool, PoolConfig, PoolEvent};
use nmp_network::role::RelayRole;
use serde_json::json;
use std::collections::HashMap;
use std::sync::mpsc;

const UNSEEN_RELAY: &str = "ws://127.0.0.1:1/";
const CANONICAL_UNSEEN_RELAY: &str = "ws://127.0.0.1:1";

fn signed_raw_event(content: &str) -> crate::store::RawEvent {
    use nostr::{EventBuilder, JsonUtil, Keys, Timestamp};

    let keys = Keys::generate();
    let event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(1_700_000_000))
        .sign_with_keys(&keys)
        .expect("sign test event");
    serde_json::from_str(&event.try_as_json().expect("event json")).expect("flat NIP-01 RawEvent")
}

fn publish_message(relay_url: &str, event_id: &str) -> OutboundMessage {
    OutboundMessage {
        role: RelayRole::Content,
        relay_url: relay_url.to_string(),
        text: json!(["EVENT", {"id": event_id}]).to_string(),
    }
}

/// Build the full actor-side transport substrate every test needs.
/// Returns `(kernel, pool, events_rx, relay_runtime)`.
/// `events_rx` is kept around so the channel doesn't disconnect mid-test.
fn route_state() -> (Kernel, Pool, mpsc::Receiver<PoolEvent>, RelayRuntime) {
    let (events_tx, events_rx) = mpsc::channel::<PoolEvent>();
    let pool = Pool::new(PoolConfig::default(), events_tx);
    (
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
        pool,
        events_rx,
        RelayRuntime::new(),
    )
}

#[test]
fn explicit_publish_target_spawns_worker_for_unseen_relay() {
    let (mut kernel, pool, _events_rx, mut rt) = route_state();
    let raw = signed_raw_event("explicit relay dispatch");
    let outbound = publish_signed_event(
        &mut kernel,
        raw,
        PublishTarget::Explicit {
            relays: vec![UNSEEN_RELAY.to_string()],
        },
        None,
    );
    let mut queued_publish_outbound = Vec::new();

    route_dispatch_outbound(
        true,
        &mut queued_publish_outbound,
        &mut rt,
        &pool,
        &mut kernel,
        outbound,
    );

    assert!(
        rt.relay_controls
            .contains_key(&CanonicalRelayUrl::parse_or_raw(CANONICAL_UNSEEN_RELAY)),
        "explicit publish target must spawn a worker for its relay URL"
    );
    assert!(queued_publish_outbound.is_empty());
    close_relays(&mut rt, &pool, &mut kernel);
}

#[test]
fn create_account_publish_targets_spawn_workers_for_unseen_relays() {
    let (mut kernel, pool, _events_rx, mut rt) = route_state();
    let mut identity = IdentityRuntime::new(
        new_bunker_handshake_slot(),
        crate::actor::new_signer_state_slot(),
    );
    let relays = vec![(UNSEEN_RELAY.to_string(), "write".to_string())];
    let outbound = create_account(
        &mut identity,
        &mut kernel,
        true,
        &HashMap::new(),
        &relays,
        &[],
        false,
        true,
    );
    let mut queued_publish_outbound = Vec::new();

    route_dispatch_outbound(
        true,
        &mut queued_publish_outbound,
        &mut rt,
        &pool,
        &mut kernel,
        outbound,
    );

    assert!(
        rt.relay_controls
            .contains_key(&CanonicalRelayUrl::parse_or_raw(CANONICAL_UNSEEN_RELAY)),
        "CreateAccount cold-start publish output must spawn a worker for declared relays"
    );
    assert!(queued_publish_outbound.is_empty());
    close_relays(&mut rt, &pool, &mut kernel);
}

#[test]
fn stopped_actor_queues_publish_frames_until_running() {
    let (mut kernel, pool, _events_rx, mut rt) = route_state();
    let mut queued_publish_outbound = Vec::new();

    route_dispatch_outbound(
        false,
        &mut queued_publish_outbound,
        &mut rt,
        &pool,
        &mut kernel,
        vec![publish_message(UNSEEN_RELAY, "offline-event")],
    );

    assert!(
        rt.relay_controls.is_empty(),
        "stopped actor must not spawn workers"
    );
    assert_eq!(
        queued_publish_outbound.len(),
        1,
        "publish frame must be retained while the actor is stopped"
    );

    route_dispatch_outbound(
        true,
        &mut queued_publish_outbound,
        &mut rt,
        &pool,
        &mut kernel,
        Vec::new(),
    );

    assert!(
        queued_publish_outbound.is_empty(),
        "queued publish frame must flush once the actor is running"
    );
    assert!(
        rt.relay_controls
            .contains_key(&CanonicalRelayUrl::parse_or_raw(CANONICAL_UNSEEN_RELAY)),
        "flushed publish frame must spawn a worker for its relay URL"
    );
    close_relays(&mut rt, &pool, &mut kernel);
}
