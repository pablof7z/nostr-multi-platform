//! Actor main event loop — extracted from `actor_run.rs` to keep both files
//! within the 500-LOC hard cap (AGENTS.md §file-size).
//!
//! `run_actor_loop` owns the `loop { … }` body; `actor_run.rs` handles all
//! initialisation and then calls this function.

use super::commands::IdentityRuntime;
use super::{ActorConfig, RelayControl, GC_TICK_INTERVAL};
use super::{auth_sign, relay_event_guard};
use super::capability_worker::CapabilityWorkSender;
use super::dispatch::{dispatch_command, ActorContext};
use super::inbox::{CommandLaneDrain, Inbox, LoopStep, MailScheduler};
use super::outbound::wire_frames_to_outbound;
use super::pending_sign::{self, ParkedSignerOps, PublishObligation};
use super::relay_idle::{sweep_temporary_idle_relays, TEMPORARY_RELAY_IDLE_GRACE};
use super::relay_mgmt::{
    claim_send_gate, close_relays, maybe_send_startup, route_dispatch_outbound, send_all_outbound,
};
use super::tick::{compute_wait, emit_now, flush_due};
use super::inbox::CommandSender;
use super::commands::LifecycleObserverSlot;
use crate::capability_socket::CapabilityCallbackSlot;
use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole};
use crate::slots::{
    ActiveAccountSlot, ActiveLocalKeysSlot, EventStoreSlot, MlsLocalNsecSlot,
    PullCursorRegistryHandleSlot, RoutingTraceSlot,
};
use crate::substrate::ExternalEventSinkDispatcher;
use crate::update_envelope::UpdateFrameBytes;
use nmp_network::pool::Pool;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc::Sender, Arc};
use std::time::Instant;

/// All loop-local state threaded from `run_actor_with_observers` into the main
/// event loop.  Bundled into a struct so `run_actor_loop`'s parameter list
/// stays manageable.
pub(super) struct ActorLoopState {
    pub(super) running: bool,
    pub(super) emit_hz: u32,
    pub(super) last_emit: Instant,
    pub(super) relay_controls: HashMap<CanonicalRelayUrl, RelayControl>,
    pub(super) slot_to_url: HashMap<u32, CanonicalRelayUrl>,
    pub(super) connected_relays: HashSet<RelayRole>,
    pub(super) connected_urls: HashSet<CanonicalRelayUrl>,
    pub(super) next_relay_generation: u64,
    pub(super) last_gc: Instant,
    pub(super) startup_sent: bool,
    pub(super) parked_ops: ParkedSignerOps,
    pub(super) queued_publish_outbound: Vec<OutboundMessage>,
    pub(super) first_command: Option<super::ActorCommand>,
}

/// Runs the actor's main `loop { … }` until shutdown.
///
/// All state that was initialised in `run_actor_with_observers` before the
/// loop is passed in via `state` (loop-local mutable scalars/maps) and the
/// remaining parameters (shared handles that are never re-assigned).
#[allow(clippy::too_many_arguments)]
pub(super) fn run_actor_loop(
    state: ActorLoopState,
    mut kernel: crate::Kernel,
    inbox: Inbox,
    mut scheduler: MailScheduler,
    pool: Pool,
    update_tx: Sender<UpdateFrameBytes>,
    queue_depth: Arc<AtomicU64>,
    config: ActorConfig,
    identity: &mut IdentityRuntime,
    lifecycle_observer: &LifecycleObserverSlot,
    mls_local_nsec: &MlsLocalNsecSlot,
    active_local_keys: &ActiveLocalKeysSlot,
    capability_callback: &CapabilityCallbackSlot,
    command_tx_self: &CommandSender,
    capability_work_tx: &CapabilityWorkSender,
    routing_trace: &RoutingTraceSlot,
    event_store: &EventStoreSlot,
    pull_cursor_registry: &PullCursorRegistryHandleSlot,
    active_account: &ActiveAccountSlot,
    external_event_sink_dispatcher: &ExternalEventSinkDispatcher,
) {
    // Unpack state fields into locals for ergonomics inside the loop.
    let ActorLoopState {
        mut running,
        mut emit_hz,
        mut last_emit,
        mut relay_controls,
        mut slot_to_url,
        mut connected_relays,
        mut connected_urls,
        mut next_relay_generation,
        mut last_gc,
        mut startup_sent,
        mut parked_ops,
        mut queued_publish_outbound,
        mut first_command,
    } = state;

    loop {
        // ── Priority lane: commands ──────────────────────────────────────
        // ADR-0050 §D3a: drain a bounded burst of commands first each
        // iteration, stashing relay mail into the backlog. Returns commands
        // as a `Vec` so the mutable kernel/identity dispatch can run after.
        let CommandLaneDrain {
            commands,
            drain: command_drain,
            disconnected: inbox_disconnected,
        } = scheduler.drain_command_lane(&inbox, first_command.take());
        for command in commands {
            {
                {
                    // G-S4 — mirror NmpApp::send_cmd's fetch_add(1); saturating
                    // so the actor_sender bypass path can't underflow.
                    queue_depth
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                            Some(d.saturating_sub(1))
                        })
                        .ok();
                    // Fix A: ANY bootstrap lane connected → relays ready
                    // (`claim_send_gate`); prior `all`-lane gate caused hangs.
                    let relays_ready = claim_send_gate(&connected_relays);
                    let mut ctx = ActorContext {
                        kernel: &mut kernel,
                        identity,
                        relay_controls: &mut relay_controls,
                        slot_to_url: &mut slot_to_url,
                        pool: &pool,
                        connected_relays: &mut connected_relays,
                        connected_urls: &mut connected_urls,
                        update_tx: &update_tx,
                        last_emit: &mut last_emit,
                        next_relay_generation: &mut next_relay_generation,
                        running: &mut running,
                        emit_hz: &mut emit_hz,
                        startup_sent: &mut startup_sent,
                        relays_ready,
                        lifecycle_observer,
                        mls_local_nsec,
                        active_local_keys,
                        capability_callback,
                        parked_ops: &mut parked_ops,
                        command_tx_self,
                        capability_work_tx,
                        config: &config,
                        routing_trace_slot: routing_trace,
                        event_store_slot: event_store,
                        pull_cursor_registry_slot: pull_cursor_registry,
                        active_account_slot: active_account,
                        external_event_sink_dispatcher,
                    };
                    let outbound = dispatch_command(command, &mut ctx);
                    let Some(outbound) = outbound else {
                        return; // Shutdown
                    };
                    route_dispatch_outbound(
                        running,
                        &mut queued_publish_outbound,
                        &mut relay_controls,
                        &mut slot_to_url,
                        &pool,
                        &mut kernel,
                        &mut next_relay_generation,
                        outbound,
                    );
                    if running
                        && maybe_send_startup(
                            running,
                            &mut startup_sent,
                            &connected_relays,
                            &mut relay_controls,
                            &mut slot_to_url,
                            &pool,
                            &mut kernel,
                            &mut next_relay_generation,
                        )
                    {
                        emit_now(&mut kernel, running, &update_tx, &mut last_emit);
                    }
                }
            }
        }
        // Inbox closed (every `CommandSender` clone dropped) → tear down. This
        // is the merged-inbox equivalent of the old `command_rx`
        // `Disconnected` arm: relay traffic alone can never disconnect the
        // inbox (the actor holds the relay sink), so a disconnect means all
        // command senders are gone.
        if inbox_disconnected {
            close_relays(
                &mut relay_controls,
                &mut slot_to_url,
                &pool,
                &mut connected_relays,
                &mut kernel,
            );
            connected_urls.clear();
            return;
        }

        // ── Relay event lane ─────────────────────────────────────────────
        // SINGLE blocking point (D8). Phase F: PoolEvent push-model. Stale
        // generations filtered in handle_relay_event via slot_to_url map.
        // panic-isolated via relay_event_guard::process_relay_event (D1).
        // Macro avoids re-listing ~13 locals at two call sites.
        macro_rules! process_relay_event {
            ($event:expr) => {
                relay_event_guard::process_relay_event(
                    $event,
                    &mut kernel,
                    &config.relay_text_interceptors,
                    &config.relay_connected_hooks,
                    command_tx_self,
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut next_relay_generation,
                    &mut connected_relays,
                    &mut connected_urls,
                    &update_tx,
                    &mut last_emit,
                    &mut startup_sent,
                    running,
                )
            };
        }

        // #1264: serve a BOUNDED batch of staged backlog events this iteration
        // (up to RELAY_BACKLOG_DRAIN_BATCH) so the backlog drains faster than a
        // sustained relay flood fills it — then ALWAYS fall through to the
        // single blocking `recv_timeout` below. A non-empty backlog therefore no
        // longer bypasses the one wait per iteration (D8), which kills the
        // busy-spin that previously pinned the CPU under flood.
        for event in scheduler.drain_backlog_batch() {
            process_relay_event!(event);
        }

        // #1264: zero wait when backlog remains so the loop keeps draining
        // promptly while still reaching the single blocking point (D8).
        let wait = if scheduler.has_backlog() {
            std::time::Duration::ZERO
        } else {
            command_drain.relay_wait(compute_wait(&kernel, running, last_emit, emit_hz))
        };
        match scheduler.next_after_drain(&inbox, wait) {
            LoopStep::Command(command) => {
                // Woken by a command during the blocking wait — replay it on
                // next iteration's priority lane (zero added latency).
                first_command = Some(command);
            }
            LoopStep::Shutdown => {
                close_relays(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut connected_relays,
                    &mut kernel,
                );
                connected_urls.clear();
                return;
            }
            LoopStep::Idle => {
                // Timeout (normal idle tick) — fall through to idle work.
            }
            LoopStep::Relay(event) => {
                process_relay_event!(event);
            }
        }

        // ── Idle work (runs on every iteration after relay poll) ─────────
        // Flush any time-gated view requests (e.g. contacts_deadline) and
        // run the M2 planner tick only while the actor is running. Before
        // Start these would spawn relay workers (via send_all_outbound) and
        // trigger relay-lifecycle events that emit spurious snapshots on the
        // update channel even though no consumer is listening — the root
        // cause of the S2 retention leak (T114b / s2-retention-audit.md).
        // The publish engine tick below already carries the same running gate
        // for the same reason. Pending profile claims, deferred view
        // requests, and lifecycle triggers all survive in kernel state until
        // Start flushes them through spawn_missing_relays + the first
        // running-gated idle tick.

        // V-64: drive wall-clock-gated sweeps (e.g. NIP-47 pending-payment
        // TTL expiry) even when no relay frame arrives. The interceptor's
        // default `on_idle_tick` is a no-op; the nmp-nip47 impl uses this
        // hook to close expired pay_invoice correlations via
        // `record_action_failure`. No running gate — sweeps must fire even
        // before Start so that entries enqueued during connection setup are
        // not orphaned if the relay never connects.
        {
            for interceptor in &config.relay_text_interceptors {
                let extra = interceptor.on_idle_tick(&mut kernel);
                if !extra.is_empty() {
                    send_all_outbound(
                        &mut relay_controls,
                        &mut slot_to_url,
                        &pool,
                        &mut kernel,
                        &mut next_relay_generation,
                        extra,
                    );
                }
            }
        }

        if running {
            let pending = kernel.pending_view_requests();
            if !pending.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    pending,
                );
            }
        }
        // T142 — M2 planner tick (after M1 pending_view_requests so CLOSE
        // frames are queued before new REQs — spec §3.1 placement rationale).
        if running {
            let wire_frames = kernel.drain_lifecycle_tick();
            if !wire_frames.is_empty() {
                let outbound = wire_frames_to_outbound(wire_frames, &mut kernel);
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    outbound,
                );
            }
        }
        // W6 — claim-expansion tick: advance per-claim Phase 1→2 state
        // machine; empty map is a no-op (D8). D4: sole writer of pending_claims.
        if running {
            let expansion_msgs = kernel.poll_claim_expansion(Instant::now());
            if !expansion_msgs.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    expansion_msgs,
                );
            }
        }
        kernel.flush_relay_scores_if_dirty();
        // T127: publish-engine tick — heap-free when nothing is in-flight
        // (D8). Retries fire on idle so T117 Residual 1 is closed.
        if running {
            let retry_frames = kernel.tick_publish_engine_for_now();
            if !retry_frames.is_empty() {
                send_all_outbound(
                    &mut relay_controls,
                    &mut slot_to_url,
                    &pool,
                    &mut kernel,
                    &mut next_relay_generation,
                    retry_frames,
                );
            }
        }
        if running {
            sweep_temporary_idle_relays(
                &mut relay_controls,
                &mut slot_to_url,
                &mut connected_urls,
                &pool,
                &mut kernel,
                Instant::now(),
                TEMPORARY_RELAY_IDLE_GRACE,
            );
        }
        // #1069 — wall-clock-gated GC pass (≤60 s, gc.md §3, D8/D9 clean).
        if running && last_gc.elapsed() >= GC_TICK_INTERVAL {
            kernel.run_gc_step();
            last_gc = Instant::now();
        }
        // ADR-0045 §5 — chunked cache-serve step; runs before flush_due
        // so served events land in this tick's snapshot (D1).
        if running && (kernel.has_pending_cache_serves() || kernel.has_cache_serve_wakeups()) {
            kernel.run_cache_serve_step();
        }
        // ── V-06 / #960: drain kernel-emitted NIP-42 AUTH signs ──────────
        // `handle_message` enqueues an AUTH kind:22242 for any relay lane whose
        // active account is a REMOTE signer; route each through the async signer
        // port (park under the `Auth` sink) — see `auth_sign::drain_pending_auth_signs`.
        auth_sign::drain_pending_auth_signs(
            &mut kernel,
            identity,
            &mut parked_ops,
            &mut auth_sign::RouteCtx {
                running,
                queued_publish_outbound: &mut queued_publish_outbound,
                relay_controls: &mut relay_controls,
                slot_to_url: &mut slot_to_url,
                pool: &pool,
                next_relay_generation: &mut next_relay_generation,
            },
        );
        // ── Unified parked-op drain (ADR-0050 §D2; #1753) ───────────────
        // One canonical drain driver shared with the wasm KernelReducer.
        // Obligations collected after drive ends (avoids overlapping borrows).
        if !parked_ops.is_empty() {
            let pending_sign::DrainBatch {
                publish: publish_obligations,
                auth: auth_obligations,
                changed: any_changed,
            } = parked_ops.drive(&mut kernel);
            // V-06 / #960: NIP-42 AUTH obligations — after drive ends.
            auth_sign::run_auth_obligations(
                &mut kernel,
                auth_obligations,
                &mut auth_sign::RouteCtx {
                    running,
                    queued_publish_outbound: &mut queued_publish_outbound,
                    relay_controls: &mut relay_controls,
                    slot_to_url: &mut slot_to_url,
                    pool: &pool,
                    next_relay_generation: &mut next_relay_generation,
                },
            );
            // Execute publish obligations from the `Publish` sink (D6).
            for obligation in publish_obligations {
                match obligation {
                    PublishObligation::Publish {
                        signed,
                        p_tags,
                        target,
                        correlation_id_override,
                    } => {
                        let outbound = kernel.publish_signed_to_with_correlation(
                            &signed,
                            &p_tags,
                            target,
                            correlation_id_override,
                        );
                        route_dispatch_outbound(
                            running,
                            &mut queued_publish_outbound,
                            &mut relay_controls,
                            &mut slot_to_url,
                            &pool,
                            &mut kernel,
                            &mut next_relay_generation,
                            outbound,
                        );
                    }
                    PublishObligation::Failed {
                        toast,
                        correlation_id_override,
                        reason_code,
                    } => {
                        kernel.set_last_error_toast(Some(toast.clone()));
                        // Recorded BEFORE `emit_now` (below) so this tick's
                        // snapshot drains it; `None` (a `react` / `follow` park)
                        // is a no-op — nothing is waiting on an id. A
                        // capability/signer denial carries the curated
                        // `reason_code` (S7, #1754) so the host localizes the
                        // failure; an un-coded failure stays prose-only.
                        if let Some(id) = correlation_id_override {
                            kernel.record_action_failure_coded(id, toast, reason_code, None);
                        }
                    }
                }
            }
            // Surface the changes immediately rather than waiting up to one
            // periodic flush tick — matches the prior per-op `emit_now`.
            if any_changed && running {
                emit_now(&mut kernel, running, &update_tx, &mut last_emit);
            }
        }
        // Only emit when state actually changed; do not emit on every
        // idle tick (D8: zero false-wakeup allocations after warmup).
        if flush_due(&kernel, running, last_emit, emit_hz) {
            emit_now(&mut kernel, running, &update_tx, &mut last_emit);
        }
    }
}
