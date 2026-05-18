//! Actor main loop — message routing, command dispatch, relay event handling.
//!
//! Idle-tick timing helpers are in `tick.rs`.
//! Relay lifecycle helpers are in `relay_mgmt.rs`.

mod relay_mgmt;
mod tick;

use relay_mgmt::{
    all_relays_connected, bridge_commands, bridge_relays, close_relays, maybe_send_startup,
    send_all_outbound, spawn_missing_relays,
};
use tick::{emit_now, flush_due, next_actor_msg};

use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use crate::relay_worker::{RelayCommand, RelayEvent};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// Actor command variants.  The `actor` module is private (`mod actor`, not
/// `pub mod actor`), so this `pub` is only reachable from outside the crate
/// through the `testing` re-export gate.  In normal (non-test-support) builds
/// nothing re-exports these items, so they remain effectively crate-private.
#[derive(Debug)]
pub enum ActorCommand {
    Start { visible_limit: usize, emit_hz: u32 },
    Configure { visible_limit: usize, emit_hz: u32 },
    OpenAuthor { pubkey: String },
    OpenThread { event_id: String },
    OpenFirehoseTag { tag: String },
    ClaimProfile { pubkey: String, consumer_id: String },
    ReleaseProfile { pubkey: String, consumer_id: String },
    CloseAuthor { pubkey: String },
    CloseThread { event_id: String },
    Stop,
    Reset,
    Shutdown,
    /// Ingest pre-verified timeline events through the test-support kernel path.
    ///
    /// The caller is responsible for constructing `VerifiedEvent` values; this
    /// command routes each through `kernel::ingest_pre_verified_event` under the
    /// `"diag-firehose-stress"` sub-id. It inserts through the `EventStore`, then
    /// updates the lightweight read-cache directly. No signature re-verification
    /// is performed — the `VerifiedEvent` type is the gate.
    ///
    /// Test-support only (D0: not part of production FFI surface).
    #[cfg(any(test, feature = "test-support"))]
    IngestPreVerifiedEvents(Vec<crate::store::VerifiedEvent>),
}

pub(super) enum ActorMsg {
    Command(ActorCommand),
    Relay(RelayEvent),
}

pub(super) struct RelayControl {
    pub(super) generation: u64,
    pub(super) tx: Sender<RelayCommand>,
}

pub fn run_actor(command_rx: Receiver<ActorCommand>, update_tx: Sender<String>) {
    let (actor_tx, actor_rx) = mpsc::channel();
    bridge_commands(command_rx, actor_tx.clone());
    let (relay_tx, relay_rx) = mpsc::channel();
    bridge_relays(relay_rx, actor_tx.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut relay_controls: HashMap<RelayRole, RelayControl> = HashMap::new();
    let mut connected_relays = HashSet::new();
    let mut next_relay_generation = 1;
    let mut running = false;
    let mut emit_hz = DEFAULT_EMIT_HZ;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut startup_sent = false;

    loop {
        let message = match next_actor_msg(&actor_rx, &kernel, running, last_emit, emit_hz) {
            Ok(Some(message)) => message,
            Ok(None) => {
                // Flush any time-gated view requests (e.g. contacts_deadline).
                let pending = kernel.pending_view_requests();
                if !pending.is_empty() {
                    send_all_outbound(&relay_controls, &mut kernel, pending);
                }
                // Only emit when state actually changed; do not emit on every
                // idle tick (D8: zero false-wakeup allocations after warmup).
                if kernel.changed_since_emit() {
                    emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                }
                continue;
            }
            Err(()) => {
                close_relays(&mut relay_controls, &mut connected_relays, &mut kernel);
                return;
            }
        };

        match message {
            ActorMsg::Command(command) => {
                let relays_ready = all_relays_connected(&connected_relays);
                let outbound = dispatch_command(
                    command,
                    &mut kernel,
                    &mut relay_controls,
                    &relay_tx,
                    &mut connected_relays,
                    &update_tx,
                    &mut last_emit,
                    &mut next_relay_generation,
                    &mut running,
                    &mut emit_hz,
                    &mut startup_sent,
                    relays_ready,
                );
                let Some(outbound) = outbound else {
                    return; // Shutdown
                };
                if running {
                    send_all_outbound(&relay_controls, &mut kernel, outbound);
                    if maybe_send_startup(
                        running,
                        &mut startup_sent,
                        &connected_relays,
                        &relay_controls,
                        &mut kernel,
                    ) {
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                }
            }
            ActorMsg::Relay(event) => {
                let role = event.role();
                let generation = event.generation();
                if relay_controls
                    .get(&role)
                    .is_none_or(|control| control.generation != generation)
                {
                    continue;
                }
                handle_relay_event(
                    event,
                    &mut kernel,
                    &mut relay_controls,
                    &mut connected_relays,
                    &update_tx,
                    &mut last_emit,
                    &mut startup_sent,
                    running,
                );
            }
        }

        if flush_due(&kernel, running, last_emit, emit_hz) {
            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_command(
    command: ActorCommand,
    kernel: &mut Kernel,
    relay_controls: &mut HashMap<RelayRole, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    connected_relays: &mut HashSet<RelayRole>,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
    next_relay_generation: &mut u64,
    running: &mut bool,
    emit_hz: &mut u32,
    startup_sent: &mut bool,
    relays_ready: bool,
) -> Option<Vec<OutboundMessage>> {
    match command {
        ActorCommand::Start {
            visible_limit,
            emit_hz: hz,
        } => {
            *running = true;
            *emit_hz = hz;
            *startup_sent = false;
            kernel.set_visible_limit(visible_limit);
            kernel.start();
            spawn_missing_relays(relay_controls, relay_tx, kernel, next_relay_generation);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Configure {
            visible_limit,
            emit_hz: hz,
        } => {
            *emit_hz = hz;
            kernel.set_visible_limit(visible_limit);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::OpenAuthor { pubkey } => {
            let outbound = kernel.open_author(pubkey, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::OpenThread { event_id } => {
            let outbound = kernel.open_thread(event_id, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::OpenFirehoseTag { tag } => {
            let outbound = kernel.open_firehose_tag(tag, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::ClaimProfile {
            pubkey,
            consumer_id,
        } => {
            let outbound = kernel.claim_profile(pubkey, consumer_id, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::ReleaseProfile {
            pubkey,
            consumer_id,
        } => {
            let outbound = kernel.release_profile(&pubkey, &consumer_id);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::CloseAuthor { pubkey } => {
            let outbound = kernel.close_author(&pubkey);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::CloseThread { event_id } => {
            let outbound = kernel.close_thread(&event_id);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::Stop => {
            *running = false;
            *startup_sent = false;
            close_relays(relay_controls, connected_relays, kernel);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Reset => {
            close_relays(relay_controls, connected_relays, kernel);
            *kernel = Kernel::new(kernel.visible_limit());
            *startup_sent = false;
            if *running {
                kernel.start();
                spawn_missing_relays(relay_controls, relay_tx, kernel, next_relay_generation);
            }
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Shutdown => {
            close_relays(relay_controls, connected_relays, kernel);
            None
        }
        #[cfg(any(test, feature = "test-support"))]
        ActorCommand::IngestPreVerifiedEvents(events) => {
            // D4 (single writer per fact): actor thread is the sole mutator.
            // Routes each event through kernel.ingest_pre_verified_event under the
            // "diag-firehose-stress" sub-id.  Note: ingest_pre_verified_event does
            // NOT call should_store_event or ingest_timeline_event — it directly
            // calls store.insert + populates the read-cache (events HashMap + timeline).
            // sort_timeline() is deferred to after the loop to avoid O(n²·log n)
            // cost for large batches (e.g. S3: 100k events).
            for verified in events {
                kernel.ingest_pre_verified_event(
                    crate::relay::RelayRole::Content,
                    "diag-firehose-stress",
                    verified,
                );
            }
            // One sort after all events are ingested: O(n log n) not O(n²·log n).
            kernel.sort_timeline_deferred();
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_relay_event(
    event: RelayEvent,
    kernel: &mut Kernel,
    relay_controls: &mut HashMap<RelayRole, RelayControl>,
    connected_relays: &mut HashSet<RelayRole>,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
    startup_sent: &mut bool,
    running: bool,
) {
    match event {
        RelayEvent::Connected { role, .. } => {
            connected_relays.insert(role);
            kernel.relay_connected(role);
            maybe_send_startup(
                running,
                startup_sent,
                connected_relays,
                relay_controls,
                kernel,
            );
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Failed { role, error, .. } => {
            connected_relays.remove(&role);
            *startup_sent = false;
            kernel.relay_failed(role, error);
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Closed { role, .. } => {
            connected_relays.remove(&role);
            *startup_sent = false;
            kernel.relay_closed(role);
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Message { role, message, .. } if running => {
            let mut outbound = kernel.handle_message(role, message);
            outbound.extend(kernel.pending_view_requests());
            send_all_outbound(relay_controls, kernel, outbound);
        }
        RelayEvent::Message { .. } => {}
    }
}
