//! Publish-output relay dispatch regression coverage.
//!
//! These tests stay at the actor relay boundary: the publish engine/commands
//! produce `OutboundMessage`s with concrete relay URLs, and relay lifecycle code
//! must either spawn a worker immediately or retain publish frames until the
//! actor is running again.

use super::commands::{create_account, publish_signed_event, IdentityRuntime};
use super::relay_mgmt::{close_relays, route_dispatch_outbound};
use super::RelayControl;
use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::relay_worker::RelayEvent;
use serde_json::json;
use std::collections::{HashMap, HashSet};
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

fn route_state() -> (
    Kernel,
    mpsc::Sender<RelayEvent>,
    HashMap<CanonicalRelayUrl, RelayControl>,
    u64,
) {
    let (relay_tx, _relay_rx) = mpsc::channel::<RelayEvent>();
    (
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
        relay_tx,
        HashMap::new(),
        1,
    )
}

#[test]
fn explicit_publish_target_spawns_worker_for_unseen_relay() {
    let (mut kernel, relay_tx, mut relay_controls, mut next_generation) = route_state();
    let raw = signed_raw_event("explicit relay dispatch");
    let outbound = publish_signed_event(&mut kernel, raw, &[UNSEEN_RELAY.to_string()]);
    let mut queued_publish_outbound = Vec::new();

    route_dispatch_outbound(
        true,
        &mut queued_publish_outbound,
        &mut relay_controls,
        &relay_tx,
        &mut kernel,
        &mut next_generation,
        outbound,
    );

    assert!(
        relay_controls.contains_key(&CanonicalRelayUrl::parse_or_raw(CANONICAL_UNSEEN_RELAY)),
        "explicit publish target must spawn a worker for its relay URL"
    );
    assert!(queued_publish_outbound.is_empty());
    close_relays(&mut relay_controls, &mut HashSet::new(), &mut kernel);
}

#[test]
fn create_account_publish_targets_spawn_workers_for_unseen_relays() {
    let (mut kernel, relay_tx, mut relay_controls, mut next_generation) = route_state();
    let mut identity = IdentityRuntime::new();
    let relays = vec![(UNSEEN_RELAY.to_string(), "write".to_string())];
    let outbound = create_account(&mut identity, &mut kernel, true, &HashMap::new(), &relays, false);
    let mut queued_publish_outbound = Vec::new();

    route_dispatch_outbound(
        true,
        &mut queued_publish_outbound,
        &mut relay_controls,
        &relay_tx,
        &mut kernel,
        &mut next_generation,
        outbound,
    );

    assert!(
        relay_controls.contains_key(&CanonicalRelayUrl::parse_or_raw(CANONICAL_UNSEEN_RELAY)),
        "CreateAccount cold-start publish output must spawn a worker for declared relays"
    );
    assert!(queued_publish_outbound.is_empty());
    close_relays(&mut relay_controls, &mut HashSet::new(), &mut kernel);
}

#[test]
fn stopped_actor_queues_publish_frames_until_running() {
    let (mut kernel, relay_tx, mut relay_controls, mut next_generation) = route_state();
    let mut queued_publish_outbound = Vec::new();

    route_dispatch_outbound(
        false,
        &mut queued_publish_outbound,
        &mut relay_controls,
        &relay_tx,
        &mut kernel,
        &mut next_generation,
        vec![publish_message(UNSEEN_RELAY, "offline-event")],
    );

    assert!(
        relay_controls.is_empty(),
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
        &mut relay_controls,
        &relay_tx,
        &mut kernel,
        &mut next_generation,
        Vec::new(),
    );

    assert!(
        queued_publish_outbound.is_empty(),
        "queued publish frame must flush once the actor is running"
    );
    assert!(
        relay_controls.contains_key(&CanonicalRelayUrl::parse_or_raw(CANONICAL_UNSEEN_RELAY)),
        "flushed publish frame must spawn a worker for its relay URL"
    );
    close_relays(&mut relay_controls, &mut HashSet::new(), &mut kernel);
}
