//! Actor main loop — message routing, command dispatch, relay event handling.
//!
//! Idle-tick timing helpers are in `tick.rs`.
//! Relay lifecycle helpers are in `relay_mgmt.rs`.

mod commands;
mod dispatch;
mod kernel_action;
mod relay_mgmt;
mod tick;

use commands::IdentityRuntime;
use dispatch::{dispatch_command, handle_relay_event};

use crate::app::KernelAction;

use relay_mgmt::{
    all_relays_connected, bridge_commands, bridge_relays, close_relays, maybe_send_startup,
    send_all_outbound,
};
use tick::{emit_now, flush_due, next_actor_msg};

use crate::kernel::Kernel;
use crate::relay::{RelayRole, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
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
    /// T66a identity — import an nsec/hex secret, add to the actor-local
    /// identity store, bind it as the active signer, retarget the timeline.
    SignInNsec { secret: String },
    /// T66a identity — parse a `bunker://` NIP-46 URI. Transport is NOT yet
    /// wired (D0 forbids `nmp-core -> nmp-signers`); this validates the URI
    /// shape and surfaces a `last_error_toast` directing the user to nsec.
    SignInBunker { uri: String },
    /// T66a identity — generate a fresh keypair and sign in with it.
    CreateAccount,
    /// T66a identity — switch the active account (synchronous re-bind +
    /// timeline retarget, mirrors AccountManager::switch_active semantics).
    SwitchActive { identity_id: String },
    /// T66a identity — remove an account; clears the active slot if it was
    /// the active one.
    RemoveAccount { identity_id: String },
    /// T66a publish — sign a kind:1 (optionally a reply) with the active
    /// account and emit it to the NIP-65 outbox-resolved write relays (D3).
    PublishNote {
        content: String,
        reply_to_id: Option<String>,
    },
    /// T66a publish — kind:7 reaction to `target_event_id`.
    React {
        target_event_id: String,
        reaction: String,
    },
    /// T66a publish — append `pubkey` to the active account's kind:3 follow
    /// set and re-publish it.
    Follow { pubkey: String },
    /// T66a publish — remove `pubkey` from the kind:3 follow set.
    Unfollow { pubkey: String },
    /// T66a relay edit — add a relay row (role: `read` | `write` | `both`).
    AddRelay { url: String, role: String },
    /// T66a relay edit — remove a relay row.
    RemoveRelay { url: String },
    /// T66a — (re)open the following-timeline for the active account.
    OpenTimeline,
    ClaimProfile { pubkey: String, consumer_id: String },
    ReleaseProfile { pubkey: String, consumer_id: String },
    CloseAuthor { pubkey: String },
    CloseThread { event_id: String },
    Stop,
    Reset,
    Shutdown,
    /// Generic FFI-boundary action (T95). Routed through the
    /// [`dispatch_kernel_action`] reducer; the resolved [`KernelUpdate`] is
    /// serialized and pushed on the update channel. `OpenUri` registers the
    /// resolved interest through the single-writer registry (D4).
    Kernel(KernelAction),
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

/// One per-URL relay-worker handle. T105: `relay_url` (NOT `role`) is the
/// pool key — every resolved write/read relay gets its own socket. `role`
/// is retained so the actor can route diagnostic-bucket updates back to
/// the kernel's lane-keyed RelayHealth rows until per-URL health lands (M11).
pub(super) struct RelayControl {
    pub(super) generation: u64,
    #[allow(dead_code)] // Diagnostic lane label; per-URL health is M11.
    pub(super) role: RelayRole,
    #[allow(dead_code)] // The URL this worker dials — the routing key in the pool.
    pub(super) relay_url: String,
    pub(super) tx: Sender<RelayCommand>,
}

pub fn run_actor(command_rx: Receiver<ActorCommand>, update_tx: Sender<String>) {
    let (actor_tx, actor_rx) = mpsc::channel();
    bridge_commands(command_rx, actor_tx.clone());
    let (relay_tx, relay_rx) = mpsc::channel();
    bridge_relays(relay_rx, actor_tx.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut identity = IdentityRuntime::new();
    // T105: URL-keyed transport pool. One socket per resolved relay URL;
    // workers spawn on demand as OutboundMessages flow with new relay_urls.
    let mut relay_controls: HashMap<String, RelayControl> = HashMap::new();
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
                    send_all_outbound(
                        &mut relay_controls,
                        &relay_tx,
                        &mut kernel,
                        &mut next_relay_generation,
                        pending,
                    );
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
                    &mut identity,
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
                    send_all_outbound(
                        &mut relay_controls,
                        &relay_tx,
                        &mut kernel,
                        &mut next_relay_generation,
                        outbound,
                    );
                    if maybe_send_startup(
                        running,
                        &mut startup_sent,
                        &connected_relays,
                        &mut relay_controls,
                        &relay_tx,
                        &mut kernel,
                        &mut next_relay_generation,
                    ) {
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                }
            }
            ActorMsg::Relay(event) => {
                let relay_url = event.relay_url().to_string();
                let generation = event.generation();
                if relay_controls
                    .get(&relay_url)
                    .is_none_or(|control| control.generation != generation)
                {
                    // Stale event from a disposed worker — ignore.
                    continue;
                }
                handle_relay_event(
                    event,
                    &mut kernel,
                    &mut relay_controls,
                    &relay_tx,
                    &mut next_relay_generation,
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
