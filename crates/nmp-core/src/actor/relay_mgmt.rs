//! Relay lifecycle helpers — spawning, closing, routing outbound messages.

use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole};
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

pub(super) fn all_relays_connected(connected_relays: &HashSet<RelayRole>) -> bool {
    RelayRole::all()
        .into_iter()
        .all(|role| connected_relays.contains(&role))
}

pub(super) fn spawn_missing_relays(
    relay_controls: &mut HashMap<RelayRole, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) {
    for role in RelayRole::all() {
        relay_controls.entry(role).or_insert_with(|| {
            let generation = *next_relay_generation;
            *next_relay_generation = generation.saturating_add(1);
            kernel.relay_connecting(role);
            RelayControl {
                generation,
                tx: spawn_relay_worker(role, generation, relay_tx.clone()),
            }
        });
    }
}

pub(super) fn maybe_send_startup(
    running: bool,
    startup_sent: &mut bool,
    connected_relays: &HashSet<RelayRole>,
    relay_controls: &HashMap<RelayRole, RelayControl>,
    kernel: &mut Kernel,
) -> bool {
    if !running || *startup_sent || !all_relays_connected(connected_relays) {
        return false;
    }

    let startup_requests = kernel.startup_requests();
    send_all_outbound(relay_controls, kernel, startup_requests);
    let view_requests = kernel.pending_view_requests();
    send_all_outbound(relay_controls, kernel, view_requests);
    *startup_sent = true;
    true
}

pub(super) fn send_all_outbound(
    relay_controls: &HashMap<RelayRole, RelayControl>,
    kernel: &mut Kernel,
    outbound: Vec<OutboundMessage>,
) {
    // M5+M2+M8 wiring: every outbound batch passes through the AUTH-pause
    // partition before hitting the wire. REQs targeting an AUTH-paused
    // relay (ChallengeReceived / Authenticating) are diverted into the
    // deferred queue and replayed on the next tick after Authenticated. This
    // is the single choke point — view-open paths (open_author, open_thread,
    // claim_profile, …) all route through here, so kernel-level partitioning
    // catches every REQ regardless of which kernel method built it.
    let outbound = kernel.partition_auth_paused(outbound);
    for message in outbound {
        send_outbound(relay_controls, kernel, message);
    }
}

pub(super) fn send_outbound(
    relay_controls: &HashMap<RelayRole, RelayControl>,
    kernel: &mut Kernel,
    message: OutboundMessage,
) {
    let Some(control) = relay_controls.get(&message.role) else {
        kernel.defer_outbound(message);
        return;
    };

    kernel.record_tx(message.role, message.text.len());
    if control.tx.send(RelayCommand::Send(message.text)).is_err() {
        kernel.relay_failed(message.role, "relay worker stopped".to_string());
    }
}

pub(super) fn close_relays(
    relay_controls: &mut HashMap<RelayRole, RelayControl>,
    connected_relays: &mut HashSet<RelayRole>,
    kernel: &mut Kernel,
) {
    for role in RelayRole::all() {
        if let Some(control) = relay_controls.remove(&role) {
            for sub_id in kernel.active_subscriptions(role) {
                let close = json!(["CLOSE", sub_id]).to_string();
                let _ = control.tx.send(RelayCommand::Send(close));
            }
            let _ = control.tx.send(RelayCommand::Shutdown);
        }
        connected_relays.remove(&role);
        kernel.relay_closed(role);
    }
}
