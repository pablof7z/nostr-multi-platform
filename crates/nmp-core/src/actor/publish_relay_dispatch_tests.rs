//! Actor-outbound relay dispatch regression coverage.
//!
//! These tests stay at the actor relay boundary: commands (publish AND
//! interest-registration alike) produce `OutboundMessage`s with concrete
//! relay URLs, and relay lifecycle code must either spawn a pool worker
//! immediately or retain the frame until the actor is running again.
//!
//! Phase F: post-cut-over the actor's per-URL transport is a
//! [`nmp_network::pool::Pool`]; these tests construct a fresh pool the same
//! way the actor runtime does and assert the bookkeeping invariants survive.
//!
//! chirp#130 added the `stopped_actor_queues_non_publish_frames_...` test:
//! before that fix, `route_dispatch_outbound`'s `!running` branch only
//! retained publish `EVENT` frames, silently dropping REQ/CLOSE frames
//! produced by a command (e.g. `InterestsCommand::OpenObservedInterest`)
//! dispatched before `Start`. See that test and the `route_dispatch_outbound`
//! doc comment in `relay_mgmt.rs` for the full mechanism.

use super::commands::{
    create_account, new_bunker_handshake_slot, publish_signed_event, IdentityRuntime,
};
use super::dispatch::build_open_interest;
use super::relay_mgmt::{close_relays, route_dispatch_outbound};
use super::relay_runtime::RelayRuntime;
use crate::kernel::Kernel;
use crate::publish::{PublishRouteClass, PublishTarget};
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
        PublishTarget::explicit(
            vec![UNSEEN_RELAY.to_string()],
            PublishRouteClass::ImportedOrPresigned,
        ),
        None,
    );
    let mut queued_actor_outbound = Vec::new();

    route_dispatch_outbound(
        true,
        &mut queued_actor_outbound,
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
    assert!(queued_actor_outbound.is_empty());
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
    let mut queued_actor_outbound = Vec::new();

    route_dispatch_outbound(
        true,
        &mut queued_actor_outbound,
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
    assert!(queued_actor_outbound.is_empty());
    close_relays(&mut rt, &pool, &mut kernel);
}

#[test]
fn stopped_actor_queues_publish_frames_until_running() {
    let (mut kernel, pool, _events_rx, mut rt) = route_state();
    let mut queued_actor_outbound = Vec::new();

    route_dispatch_outbound(
        false,
        &mut queued_actor_outbound,
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
        queued_actor_outbound.len(),
        1,
        "publish frame must be retained while the actor is stopped"
    );

    route_dispatch_outbound(
        true,
        &mut queued_actor_outbound,
        &mut rt,
        &pool,
        &mut kernel,
        Vec::new(),
    );

    assert!(
        queued_actor_outbound.is_empty(),
        "queued publish frame must flush once the actor is running"
    );
    assert!(
        rt.relay_controls
            .contains_key(&CanonicalRelayUrl::parse_or_raw(CANONICAL_UNSEEN_RELAY)),
        "flushed publish frame must spawn a worker for its relay URL"
    );
    close_relays(&mut rt, &pool, &mut kernel);
}

/// Regression test for chirp#130.
///
/// Symptom: opening an already-joined public group-chat relay's group
/// "chirp-demo" and sending a message failed with "Couldn't reach any
/// relay"; the group's host relay NEVER appeared in the live relay set for
/// the whole session. Root cause was
/// in NMP, not chirp: `Kernel::drain_lifecycle_outbound` compiles a
/// newly-registered interest into a wire REQ exactly once, synchronously,
/// regardless of `running` (`open_interest`/`open_observed_interest` in
/// `actor/dispatch/cmd_interests.rs` call it unconditionally right after
/// installing the interest). That one-shot compile also records
/// `last_compile_fingerprint` (`subs/recompile.rs`). `chirp`'s
/// `KernelModel.init()` constructs `GroupChatStore` — which FFI-registers a
/// relay-pinned observed interest for the group — synchronously, BEFORE
/// `.task { model.start() }` calls `kernel.start()`. If the REQ produced by
/// that pre-start compile is dropped instead of retained, NO later idle-tick
/// recompile ever resends it, because the planner's compile inputs never
/// change again — the memo guard in `recompile_inner` short-circuits every
/// subsequent tick with an empty diff. The relay is therefore never dialed,
/// for the entire process lifetime.
#[test]
fn stopped_actor_queues_non_publish_frames_and_flushes_once_running() {
    let (mut kernel, pool, _events_rx, mut rt) = route_state();

    const PINNED_RELAY: &str = "wss://relay.example-groups.test/";
    const CANONICAL_PINNED_RELAY: &str = "wss://relay.example-groups.test";

    // Register an interest pinned to a specific relay — mirrors chirp's
    // group-chat host-relay `relay_pin` usage (the app-side read-session
    // opener carves this out of `nmp-core`, per D0), which bypasses NIP-65
    // outbox routing so the compiled REQ deterministically targets exactly
    // one relay regardless of any other actor/mailbox configuration.
    let (identity, interest) = build_open_interest(
        r##"{"kinds":[9,10,11,12],"#h":["chirp-demo"]}"##,
        "group-chat-chirp-demo",
        0,
        Some(PINNED_RELAY),
        false,
        crate::planner::InterestLifecycle::Tailing,
    )
    .expect("valid NIP-01 filter");
    assert!(
        kernel.open_interest_sub(identity, interest),
        "first open installs the slot and enqueues a compile trigger"
    );

    // Drain the lifecycle immediately — this is EXACTLY what
    // `open_interest`/`open_observed_interest` do at command-dispatch time,
    // unconditionally, regardless of `running` (`cmd_interests.rs`).
    let outbound = kernel.drain_lifecycle_outbound();
    assert_eq!(
        outbound.len(),
        1,
        "the newly-registered interest must compile into exactly one REQ"
    );
    assert_eq!(outbound[0].relay_url, CANONICAL_PINNED_RELAY);
    assert!(
        outbound[0].text.starts_with(r#"["REQ""#),
        "compiled frame must be a REQ, got: {}",
        outbound[0].text
    );

    // Route it as if dispatched BEFORE `Start` — `running == false`, exactly
    // the window chirp's `KernelModel.init()` constructs `GroupChatStore` in,
    // before `.task { model.start() }` runs.
    let mut queued_actor_outbound = Vec::new();
    route_dispatch_outbound(
        false,
        &mut queued_actor_outbound,
        &mut rt,
        &pool,
        &mut kernel,
        outbound,
    );

    assert!(
        rt.relay_controls.is_empty(),
        "a stopped actor must not dial any relay yet"
    );
    assert_eq!(
        queued_actor_outbound.len(),
        1,
        "the compiled REQ must be RETAINED while the actor is stopped, not \
         dropped — dropping it here is a PERMANENT loss (see the next \
         assertion)"
    );

    // Prove the "no natural recovery" half of the bug: a later idle-tick
    // recompile (standing in for the tick that runs once `running` flips
    // true) finds NOTHING to send, because the interest's compile inputs
    // have not changed since the one-shot compile above — the memo guard in
    // `subs/recompile.rs` short-circuits with an empty diff. If the earlier
    // REQ had been dropped instead of queued, this proves it would be gone
    // for good, with no other mechanism able to resend it.
    let idle_tick_frames = kernel.drain_lifecycle_tick();
    assert!(
        idle_tick_frames.is_empty(),
        "a stale-input recompile must not magically resend the REQ — proving \
         the queue (not a later recompile) is the only thing that can \
         deliver it"
    );

    // Simulate `Start`: `running` flips true, and the actor's very next
    // `route_dispatch_outbound` call (Start's own dispatch — see
    // `loop_context.rs`) must flush the queue.
    route_dispatch_outbound(
        true,
        &mut queued_actor_outbound,
        &mut rt,
        &pool,
        &mut kernel,
        Vec::new(),
    );

    assert!(
        queued_actor_outbound.is_empty(),
        "queued REQ must flush once the actor starts running"
    );
    assert!(
        rt.relay_controls
            .contains_key(&CanonicalRelayUrl::parse_or_raw(CANONICAL_PINNED_RELAY)),
        "chirp#130: the pinned relay must be dialed once Start flushes the \
         queued REQ — this is the exact symptom the issue reports (\"the \
         group's host relay is NEVER present in the live relay set\")"
    );

    close_relays(&mut rt, &pool, &mut kernel);
}
