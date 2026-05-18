//! Relay lifecycle helpers — spawning, closing, routing outbound messages.
//!
//! # T105 — URL-keyed transport pool
//!
//! `relay_controls` is keyed by **resolved relay URL**, not by `RelayRole`.
//! `send_outbound` dispatches each `OutboundMessage` by its `relay_url`, and
//! a worker is spawned **on demand** the first time a new URL appears (cold
//! discovery seed at startup, then per resolved write/read relay as the
//! kernel resolves NIP-65 mailboxes). `connected_relays` is still per-`RelayRole`
//! to drive the diagnostic surface (one row per lane) until M11 makes
//! per-URL health a first-class part of the FFI projection.

use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole, BOOTSTRAP_DISCOVERY_RELAYS};
use crate::relay_worker::{spawn_relay_worker, RelayCommand, RelayEvent};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;

use super::{ActorCommand, ActorMsg, RelayControl};

pub(super) fn bridge_commands(command_rx: Receiver<ActorCommand>, actor_tx: Sender<ActorMsg>) {
    thread::spawn(move || {
        while let Ok(command) = command_rx.recv() {
            if actor_tx.send(ActorMsg::Command(command)).is_err() {
                break;
            }
        }
    });
}

pub(super) fn bridge_relays(relay_rx: Receiver<RelayEvent>, actor_tx: Sender<ActorMsg>) {
    thread::spawn(move || {
        while let Ok(event) = relay_rx.recv() {
            if actor_tx.send(ActorMsg::Relay(event)).is_err() {
                break;
            }
        }
    });
}

/// True when at least one URL on **every** lane has reported `Connected`.
/// Used as the startup-send gate so the first burst of REQs has somewhere to
/// land. Per-lane (`RelayRole`) granularity matches the diagnostic surface;
/// M11 will sharpen this to per-URL once the FFI projection lands.
pub(super) fn all_relays_connected(connected_relays: &HashSet<RelayRole>) -> bool {
    RelayRole::all()
        .into_iter()
        .all(|role| connected_relays.contains(&role))
}

/// Lane-bootstrap seeds: spawn one worker per `BOOTSTRAP_DISCOVERY_RELAYS`
/// entry mapped to its `RelayRole`. Called from `Start` so the cold-start
/// kind:10002 discovery fetch has a socket to leave on before any NIP-65
/// list is cached. Per-author/recipient sockets spawn on demand in
/// `send_outbound` as the kernel emits OutboundMessages targeting their
/// resolved relay URLs.
pub(super) fn spawn_missing_relays(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) {
    for role in RelayRole::all() {
        let bootstrap = role.bootstrap_url().to_string();
        ensure_relay_worker(
            relay_controls,
            relay_tx,
            kernel,
            next_relay_generation,
            role,
            bootstrap,
        );
    }
}

/// Spawn (if missing) a worker for `(role, relay_url)` and stamp the kernel's
/// per-role health row as `connecting`. Returns true iff a new worker was
/// spawned (the URL was previously unseen). On-demand path: any
/// `OutboundMessage` carrying a URL the pool has never seen gets a fresh
/// socket here before `send_outbound` enqueues the frame.
pub(super) fn ensure_relay_worker(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    role: RelayRole,
    relay_url: String,
) -> bool {
    if relay_controls.contains_key(&relay_url) {
        return false;
    }
    let generation = *next_relay_generation;
    *next_relay_generation = generation.saturating_add(1);
    kernel.relay_connecting(role);
    relay_controls.insert(
        relay_url.clone(),
        RelayControl {
            generation,
            role,
            relay_url: relay_url.clone(),
            tx: spawn_relay_worker(role, relay_url, generation, relay_tx.clone()),
        },
    );
    true
}

pub(super) fn maybe_send_startup(
    running: bool,
    startup_sent: &mut bool,
    connected_relays: &HashSet<RelayRole>,
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) -> bool {
    if !running || *startup_sent || !all_relays_connected(connected_relays) {
        return false;
    }

    let startup_requests = kernel.startup_requests();
    send_all_outbound(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        startup_requests,
    );
    let view_requests = kernel.pending_view_requests();
    send_all_outbound(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        view_requests,
    );
    *startup_sent = true;
    true
}

pub(super) fn send_all_outbound(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    outbound: Vec<OutboundMessage>,
) {
    // M5+M2+M8 wiring: every outbound batch passes through the AUTH-pause
    // partition before hitting the wire. REQs targeting an AUTH-paused
    // relay (ChallengeReceived / Authenticating) are diverted into the
    // deferred queue and replayed on the next tick after Authenticated.
    let outbound = kernel.partition_auth_paused(outbound);
    for message in outbound {
        send_outbound(
            relay_controls,
            relay_tx,
            kernel,
            next_relay_generation,
            message,
        );
    }
}

/// Route one `OutboundMessage` to the worker for its `relay_url`. Spawns a
/// new worker on first sight (per-URL on-demand). The previous role-based
/// fallback (defer when role's socket is missing) is gone — every message
/// resolves a concrete URL now (T105).
pub(super) fn send_outbound(
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    message: OutboundMessage,
) {
    // Spawn on demand for any URL the pool has not seen before. The
    // diagnostic lane is `message.role`; the actual socket dials `relay_url`.
    let _spawned = ensure_relay_worker(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        message.role,
        message.relay_url.clone(),
    );

    let Some(control) = relay_controls.get(&message.relay_url) else {
        // ensure_relay_worker only fails to insert under a logic bug — defer
        // so the frame isn't dropped silently.
        kernel.defer_outbound(message);
        return;
    };

    kernel.record_tx(message.role, message.text.len());
    if control.tx.send(RelayCommand::Send(message.text)).is_err() {
        kernel.relay_failed(message.role, "relay worker stopped".to_string());
    }
}

pub(super) fn close_relays(
    relay_controls: &mut HashMap<String, RelayControl>,
    connected_relays: &mut HashSet<RelayRole>,
    kernel: &mut Kernel,
) {
    // Close every active wire-sub on every per-URL socket. The kernel's
    // `active_subscriptions(role)` enumerates WireSubs by lane — we route
    // each CLOSE to the socket the sub was opened on (URL recorded in
    // WireSub by req_for_relay).
    let active = kernel.snapshot_active_wire_subs();
    for (sub_id, relay_url) in active {
        if let Some(control) = relay_controls.get(&relay_url) {
            let close = json!(["CLOSE", sub_id]).to_string();
            let _ = control.tx.send(RelayCommand::Send(close));
        }
    }
    for (_url, control) in relay_controls.drain() {
        let _ = control.tx.send(RelayCommand::Shutdown);
    }
    // Mirror the lane-level "closed" status into the kernel diagnostics.
    let _ = bootstrap_lane_close(connected_relays, kernel);
}

/// Mark each lane as closed once all its sockets are gone (post-drain).
fn bootstrap_lane_close(
    connected_relays: &mut HashSet<RelayRole>,
    kernel: &mut Kernel,
) -> [(); 0] {
    for role in RelayRole::all() {
        connected_relays.remove(&role);
        kernel.relay_closed(role);
    }
    // Ensure cold-start bootstrap seeds re-appear in the next Start cycle.
    let _ = BOOTSTRAP_DISCOVERY_RELAYS;
    []
}
