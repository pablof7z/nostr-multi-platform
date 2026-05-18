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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Bounded capacity of the actor's internal command channel (T114 part 1 of 2).
/// D8: M10.5 S2 drain (`docs/perf/m10.5/s2-drain-analysis.md`, `d6d5400`) measured
/// ~127 B/dispatch retained on the unbounded internal channel. 4096 caps worst-case
/// retention at ~520 KiB (4096 × 127 B), well under the 1 MiB D8 budget; final
/// tuning waits for the per-dispatch retention audit in T114b.
pub(super) const BOUNDED_ACTOR_CMD_CAPACITY: usize = 4096;

// Keep `bridge_commands` / `bridge_relays` (relay_mgmt.rs, out of T114 write zone)
// reachable in the symbol graph after `run_actor` was rewritten to inline its
// forwarders. Path-as-value suppresses dead_code without touching their file.
#[allow(dead_code)]
const _BRIDGE_COMMANDS_KEEPALIVE: fn(Receiver<ActorCommand>, Sender<ActorMsg>) = bridge_commands;
#[allow(dead_code)]
const _BRIDGE_RELAYS_KEEPALIVE: fn(Receiver<RelayEvent>, Sender<ActorMsg>) = bridge_relays;

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
    /// Generic, kind-agnostic publish — take an `UnsignedEvent` already built
    /// by any protocol-crate builder (`nmp_nip23::Article`, `nmp_nip01::Note`,
    /// `nmp_reactions::Reaction`, …), sign with the active account's keys,
    /// and route through the NIP-65 outbox resolver (D3). The kernel does
    /// not inspect the kind — that's the protocol crate's concern (D0).
    ///
    /// Stepping stone toward per-protocol-crate `ActionModule` impls
    /// (`kind-wrappers.md` §8 Phase 1); deprecates kind-by-kind as those land.
    PublishUnsignedEvent(crate::substrate::UnsignedEvent),
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
    // T114 part 1: bounded internal command channel. Commands `try_send` and
    // drop on `Full` (D6 fire-and-forget — never block the FFI thread); relay
    // events use the same `SyncSender` with blocking `send` so network frames
    // are not silently dropped (backpressures onto the internal relay worker).
    let (actor_tx, actor_rx) = mpsc::sync_channel::<ActorMsg>(BOUNDED_ACTOR_CMD_CAPACITY);
    let dispatch_drops = Arc::new(AtomicU64::new(0));
    spawn_bounded_command_forwarder(command_rx, actor_tx.clone(), Arc::clone(&dispatch_drops));
    let (relay_tx, relay_rx) = mpsc::channel();
    spawn_relay_forwarder(relay_rx, actor_tx.clone());
    let _ = actor_tx; // local sender no longer needed; forwarders hold clones.
    let _ = dispatch_drops; // counter is reachable to the kernel via T114b.

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
                // T127: actor-tick for the publish engine. The 250ms idle poll
                // in `next_actor_msg` (`tick.rs`) already paces this; no
                // additional throttle (the engine's own pending_retries gate
                // skips dispatch work when nothing is due). D8 — when
                // `in_flight` is empty the tick is heap-free:
                //   - `PublishEngine::tick` collects `Vec<PublishHandle>`
                //     from an empty iterator (Rust's `FromIterator for Vec`
                //     special-cases empty → `Vec::new()`, no allocation),
                //   - `QueueDispatcher::drain` swaps in `Vec::new()` via
                //     `mem::take` (no allocation when the queue was empty),
                //   - the kernel returns `drained.into_iter().map(..).collect()`
                //     which is also heap-free for an empty source.
                // Closes Residual 1 from T117 — transient retries fire even
                // on a quiet socket (no inbound traffic).
                if running {
                    let retry_frames = kernel.tick_publish_engine_for_now();
                    if !retry_frames.is_empty() {
                        send_all_outbound(
                            &mut relay_controls,
                            &relay_tx,
                            &mut kernel,
                            &mut next_relay_generation,
                            retry_frames,
                        );
                    }
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

/// FFI → bounded-actor-channel forwarder (T114 part 1 of 2).
/// `try_send`s onto the bounded sink; on `Full` drops the command and increments
/// `dispatch_drops`. Drop-newest is the defensible policy: FFI dispatch is
/// fire-and-forget (D6) and most commands are idempotent (Open/Claim/Close) or
/// retryable from the UI (publish/follow). Coalescing ships as follow-up if
/// production drops are observed.
pub(super) fn spawn_bounded_command_forwarder(
    command_rx: Receiver<ActorCommand>,
    actor_tx: SyncSender<ActorMsg>,
    dispatch_drops: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        while let Ok(command) = command_rx.recv() {
            match actor_tx.try_send(ActorMsg::Command(command)) {
                Ok(()) => {}
                Err(TrySendError::Full(_dropped)) => {
                    // D6: FFI dispatch is fire-and-forget; never block, never
                    // surface an error across the FFI boundary. Drop the
                    // command and increment the visibility counter.
                    dispatch_drops.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
    });
}

/// Relay-event → actor-channel forwarder. Uses blocking `send`: relay events MAY
/// backpressure onto the (internal) relay-worker thread. The bounded T114 scope
/// is the FFI dispatch path only — dropping network frames would be a
/// correctness bug.
pub(super) fn spawn_relay_forwarder(
    relay_rx: Receiver<RelayEvent>,
    actor_tx: SyncSender<ActorMsg>,
) {
    thread::spawn(move || {
        while let Ok(event) = relay_rx.recv() {
            if actor_tx.send(ActorMsg::Relay(event)).is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod bounded_channel_tests {
    use super::*;

    /// T114 part 1 — synthetic full-channel flood. With NO reader draining,
    /// sending past capacity must NOT block, MUST increment `dispatch_drops`,
    /// and the actor side MUST receive exactly `capacity` messages.
    #[test]
    fn full_channel_drops_excess_and_never_blocks() {
        const CAP: usize = 4;
        const FLOOD: usize = 64;

        let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
        let (actor_tx, actor_rx) = mpsc::sync_channel::<ActorMsg>(CAP);
        let drops = Arc::new(AtomicU64::new(0));
        spawn_bounded_command_forwarder(cmd_rx, actor_tx, Arc::clone(&drops));

        let start = Instant::now();
        for _ in 0..FLOOD {
            cmd_tx
                .send(ActorCommand::Kernel(KernelAction::OpenView {
                    namespace: "profile".into(),
                    key: "pk".into(),
                }))
                .expect("FFI channel send must not block (unbounded)");
        }
        thread::sleep(Duration::from_millis(50));

        let elapsed = start.elapsed();
        assert!(elapsed < Duration::from_millis(500), "forwarder blocked: {elapsed:?}");

        let mut received = 0usize;
        while actor_rx.try_recv().is_ok() {
            received += 1;
        }
        let total_drops = drops.load(Ordering::Relaxed) as usize;
        assert_eq!(received, CAP, "bounded channel held {received}, expected {CAP}");
        assert_eq!(received + total_drops, FLOOD, "received+drops != flood");
        assert!(total_drops > 0, "expected drops under flood, got {total_drops}");
    }

    /// T114 part 1 — actor still progresses after drops. Once the actor
    /// drains the bounded channel, the forwarder's next `try_send` must
    /// succeed (no orphaned thread, no disconnect).
    #[test]
    fn actor_progresses_after_overflow_drops() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
        let (actor_tx, actor_rx) = mpsc::sync_channel::<ActorMsg>(2);
        let drops = Arc::new(AtomicU64::new(0));
        spawn_bounded_command_forwarder(cmd_rx, actor_tx, Arc::clone(&drops));

        for _ in 0..16 {
            cmd_tx.send(ActorCommand::Reset).expect("FFI send");
        }
        thread::sleep(Duration::from_millis(50));
        assert!(drops.load(Ordering::Relaxed) > 0, "expected drops after flood");

        while actor_rx.try_recv().is_ok() {} // drain
        cmd_tx.send(ActorCommand::Stop).expect("FFI send (recovered)");
        let received = (0..20).find_map(|_| {
            thread::sleep(Duration::from_millis(10));
            actor_rx.try_recv().ok()
        });
        assert!(
            matches!(received, Some(ActorMsg::Command(ActorCommand::Stop))),
            "actor must keep progressing after drop episode"
        );
    }
}
