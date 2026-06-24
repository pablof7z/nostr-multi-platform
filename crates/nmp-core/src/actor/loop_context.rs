//! Actor loop lane helpers — extracted from `run_actor_with_observers` in
//! `actor/mod.rs` to keep that file within the 500 LOC hard cap (AGENTS.md).
//!
//! # Structure
//!
//! | Symbol | Role |
//! |--------|------|
//! | [`LoopContext`] | Borrowed bundle of all mutable loop-locals shared across lanes |
//! | [`drain_commands`] | Priority command-lane drain + per-command dispatch |
//! | [`run_idle_work`] | All idle-tick maintenance (relay sweeps, GC, publish retry, parked-op drain, …) |
//!
//! Both functions are `pub(super)` — called only from `actor/mod.rs`.  No
//! semantic changes relative to the inline code they replaced.
//!
//! The relay-event lane (backlog batch + single blocking `recv_timeout`) stays
//! in `mod.rs` because the `process_relay_event!` macro must scope the
//! `&mut kernel` borrow, and moving it here would require re-threading or
//! duplicating the macro expansion sites.

use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Instant;

use nmp_network::pool::Pool;

use crate::actor::capability_worker::CapabilityWorkSender;
use crate::actor::commands::{IdentityRuntime, LifecycleObserverSlot};
use crate::actor::dispatch::{dispatch_command, ActorContext};
use crate::actor::pending_sign::{DrainBatch, ParkedSignerOps, PublishObligation};
use crate::actor::relay_idle::{sweep_temporary_idle_relays, TEMPORARY_RELAY_IDLE_GRACE};
use crate::actor::relay_mgmt::{
    close_relays, maybe_send_startup, route_dispatch_outbound, send_all_outbound,
};
use crate::actor::relay_runtime::RelayRuntime;
use crate::actor::tick::{emit_now, flush_due};
use crate::actor::{ActorCommand, ActorConfig, CommandSender, GC_TICK_INTERVAL};
use crate::capability_socket::CapabilityCallbackSlot;
use crate::kernel::Kernel;
use crate::slots::{ActiveLocalKeysSlot, MlsLocalNsecSlot};

/// Borrowed bundle of the mutable loop-locals shared between the command lane,
/// relay-event lane, and idle-work lane.
///
/// Constructed once per call to `run_actor_with_observers` and passed by
/// `&mut` reference into each extracted lane helper.  The lifetime `'a` ties
/// every field to the stack frame that owns the originals — zero heap
/// allocation, no ownership transfer.
pub(super) struct LoopContext<'a> {
    // ── Kernel & identity ────────────────────────────────────────────────
    pub(super) kernel: &'a mut Kernel,
    pub(super) identity: &'a mut IdentityRuntime,

    // ── Relay-pool state ─────────────────────────────────────────────────
    /// #1938 — URL-keyed relay runtime owner (relay_controls + slot_to_url +
    /// connected_urls + next_relay_generation). Role readiness is derived from
    /// its `connected_urls`, not a parallel role-set.
    pub(super) relay_runtime: &'a mut RelayRuntime,
    pub(super) pool: &'a Pool,

    // ── Emission & timing ────────────────────────────────────────────────
    pub(super) update_tx: &'a Sender<crate::update_envelope::UpdateFrameBytes>,
    pub(super) last_emit: &'a mut Instant,
    pub(super) last_gc: &'a mut Instant,

    // ── Actor run-state flags ────────────────────────────────────────────
    pub(super) running: &'a mut bool,
    pub(super) emit_hz: &'a mut u32,
    pub(super) startup_sent: &'a mut bool,

    // ── Observer / hook slots (shared `Arc<Mutex<…>>`) ───────────────────
    pub(super) lifecycle_observer: &'a LifecycleObserverSlot,
    pub(super) mls_local_nsec: &'a MlsLocalNsecSlot,
    pub(super) active_local_keys: &'a ActiveLocalKeysSlot,
    pub(super) capability_callback: &'a CapabilityCallbackSlot,

    // ── Parked-op queue & pending outbound ───────────────────────────────
    pub(super) parked_ops: &'a mut ParkedSignerOps,
    pub(super) queued_publish_outbound: &'a mut Vec<crate::relay::OutboundMessage>,

    // ── Cross-loop senders ───────────────────────────────────────────────
    pub(super) command_tx_self: &'a CommandSender,
    pub(super) capability_work_tx: &'a CapabilityWorkSender,

    // ── Shared config & slot references ─────────────────────────────────
    pub(super) config: &'a ActorConfig,
    pub(super) routing_trace_slot: &'a Arc<
        std::sync::Mutex<Option<Arc<crate::kernel::routing_trace::RoutingTraceProjection>>>,
    >,
    pub(super) event_store_slot: &'a crate::slots::EventStoreSlot,
    pub(super) pull_cursor_registry_slot: &'a crate::slots::PullCursorRegistryHandleSlot,
    pub(super) active_account_slot: &'a crate::slots::ActiveAccountSlot,
    pub(super) external_event_sink_dispatcher: &'a crate::substrate::ExternalEventSinkDispatcher,

    // ── G-S4 queue-depth counter ─────────────────────────────────────────
    pub(super) queue_depth: &'a Arc<std::sync::atomic::AtomicU64>,
}

/// Return value from [`drain_commands`].
///
/// The command lane may encounter a `Shutdown` command or a disconnected inbox
/// (all `CommandSender` clones dropped).  Either signal must propagate back to
/// `run_actor_with_observers` so the loop can `return`.  On `Continue` we also
/// carry `budget_hit` so the relay-event lane can decide whether to use a zero
/// wait (avoiding a 250 ms stall when a burst of commands just consumed the
/// drain budget).
pub(super) enum DrainResult {
    /// Loop should continue to the relay-event lane.
    /// `budget_hit` mirrors [`fairness::CommandDrain::hit_budget`]: when true
    /// the relay-lane wait is shortened to zero so the burst can be followed
    /// up without a full idle delay.
    Continue { budget_hit: bool },
    /// Actor should shut down: relay pool has been closed by the callee,
    /// `run_actor_with_observers` should return immediately.
    Shutdown,
}

/// Priority command-lane drain + per-command dispatch.
///
/// The caller (in `actor/mod.rs`) is responsible for calling
/// `MailScheduler::drain_command_lane` and destructuring the result — it can
/// do so because `CommandLaneDrain` is `pub(super)` relative to `actor`,
/// which `mod.rs` inhabits.  This function takes the already-destructured
/// pieces so the `pub(super)` visibility boundary is not crossed here.
///
/// On a `Shutdown` command or a disconnected inbox the relay pool is closed
/// and `DrainResult::Shutdown` is returned.  On normal completion,
/// `DrainResult::Continue { budget_hit }` is returned; the caller uses
/// `budget_hit` to shorten the relay-lane wait to zero when the drain budget
/// was saturated.
///
/// All semantics are preserved verbatim from the inline code in
/// `run_actor_with_observers`.
pub(super) fn drain_commands(
    lc: &mut LoopContext<'_>,
    commands: Vec<ActorCommand>,
    inbox_disconnected: bool,
    budget_hit: bool,
) -> DrainResult {
    for command in commands {
        // G-S4 — straddle counter: one command has left the channel
        // through `drain_command_lane`. Mirror `NmpApp::send_cmd`'s
        // `fetch_add(1)` so the depth tracks occupancy.
        // `saturating_sub` guards the (benign) race where the actor
        // drains a command sent through `actor_sender`, which
        // bypasses the increment. `Relaxed` — observability, not
        // synchronization.
        lc.queue_depth
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                Some(d.saturating_sub(1))
            })
            .ok();

        // Fix A (universal latent-bug fix): `relays_ready` is the
        // SINGLE claim/open send-gate, computed here once per dispatch
        // and fed to every consumer.  #1938 — derived from per-URL socket
        // state (`RelayRuntime::any_role_connected`): true as soon as ANY
        // connected URL exists (any role lane ready).
        let relays_ready = lc.relay_runtime.any_role_connected();
        let dispatch_now = Instant::now();
        let mut ctx = ActorContext {
            kernel: lc.kernel,
            identity: lc.identity,
            relay_runtime: lc.relay_runtime,
            pool: lc.pool,
            update_tx: lc.update_tx,
            last_emit: lc.last_emit,
            dispatch_now,
            running: lc.running,
            emit_hz: lc.emit_hz,
            startup_sent: lc.startup_sent,
            relays_ready,
            lifecycle_observer: lc.lifecycle_observer,
            mls_local_nsec: lc.mls_local_nsec,
            active_local_keys: lc.active_local_keys,
            capability_callback: lc.capability_callback,
            parked_ops: lc.parked_ops,
            command_tx_self: lc.command_tx_self,
            capability_work_tx: lc.capability_work_tx,
            config: lc.config,
            routing_trace_slot: lc.routing_trace_slot,
            event_store_slot: lc.event_store_slot,
            pull_cursor_registry_slot: lc.pull_cursor_registry_slot,
            active_account_slot: lc.active_account_slot,
            external_event_sink_dispatcher: lc.external_event_sink_dispatcher,
        };
        let outbound = dispatch_command(command, &mut ctx);
        let Some(outbound) = outbound else {
            return DrainResult::Shutdown; // Shutdown command
        };
        route_dispatch_outbound(
            *lc.running,
            lc.queued_publish_outbound,
            lc.relay_runtime,
            lc.pool,
            lc.kernel,
            outbound,
        );
        if *lc.running
            && maybe_send_startup(
                *lc.running,
                lc.startup_sent,
                lc.relay_runtime,
                lc.pool,
                lc.kernel,
                dispatch_now,
            )
        {
            emit_now(lc.kernel, *lc.running, lc.update_tx, lc.last_emit);
        }
    }

    // Inbox closed (every `CommandSender` clone dropped) → tear down.
    // Relay traffic alone can never disconnect the inbox (the actor holds the
    // relay sink), so a disconnect means all command senders are gone.
    if inbox_disconnected {
        close_relays(lc.relay_runtime, lc.pool, lc.kernel);
        return DrainResult::Shutdown;
    }

    DrainResult::Continue { budget_hit }
}

/// All idle-tick maintenance, run on every loop iteration after the relay-event lane.
///
/// Covers (in order):
/// 1. Interceptor `on_idle_tick` sweeps (NIP-47 TTL expiry, etc.)
/// 2. Pending view-request dispatch
/// 3. Subscription lifecycle tick (`drain_lifecycle_tick` / M2 planner)
/// 4. Claim-expansion state machine tick
/// 5. Relay-score flush
/// 6. Publish-engine retry tick
/// 7. Temporary-relay idle sweep
/// 8. Bounded GC pass (60 s wall-clock gate)
/// 9. Cache-serve step
/// 10. NIP-42 AUTH sign drain + obligation execution
/// 11. Parked-op drive + publish / auth obligation execution
/// 12. Periodic snapshot flush (`flush_due` / `emit_now`)
///
/// All semantics are preserved verbatim from the inline code in
/// `run_actor_with_observers`.
pub(super) fn run_idle_work(lc: &mut LoopContext<'_>) {
    // ── 1. Interceptor idle ticks ─────────────────────────────────────────
    // V-64: drive wall-clock-gated sweeps (e.g. NIP-47 pending-payment TTL
    // expiry) even when no relay frame arrives.
    for interceptor in &lc.config.relay_text_interceptors {
        let extra = interceptor.on_idle_tick(lc.kernel);
        if !extra.is_empty() {
            send_all_outbound(lc.relay_runtime, lc.pool, lc.kernel, extra);
        }
    }

    // ── 2. Pending view requests ──────────────────────────────────────────
    if *lc.running {
        let now = Instant::now();
        let pending = lc.kernel.pending_view_requests_at(now);
        if !pending.is_empty() {
            send_all_outbound(lc.relay_runtime, lc.pool, lc.kernel, pending);
        }
    }

    // ── 3. M2 planner lifecycle tick ──────────────────────────────────────
    // T142 — drain subscription lifecycle trigger inbox.  Placed after M1
    // `pending_view_requests()` so M1 CLOSE frames are enqueued before M2
    // opens new subs (spec §3.1 placement rationale).
    if *lc.running {
        let wire_frames = lc.kernel.drain_lifecycle_tick();
        if !wire_frames.is_empty() {
            let outbound = crate::actor::outbound::wire_frames_to_outbound(wire_frames, lc.kernel);
            send_all_outbound(lc.relay_runtime, lc.pool, lc.kernel, outbound);
        }
    }

    // ── 4. Claim-expansion tick ───────────────────────────────────────────
    // W6 — advance per-claim Phase 1/2/3 state machine.
    if *lc.running {
        let now = Instant::now();
        let expansion_msgs = lc.kernel.poll_claim_expansion(now);
        if !expansion_msgs.is_empty() {
            send_all_outbound(lc.relay_runtime, lc.pool, lc.kernel, expansion_msgs);
        }
    }

    // ── 5. Relay-score flush ─────────────────────────────────────────────
    lc.kernel.flush_relay_scores_if_dirty();

    // ── 6. Publish-engine retry tick ──────────────────────────────────────
    // T127: actor-tick for the publish engine.  D8 — empty queue is heap-free.
    if *lc.running {
        let retry_frames = lc.kernel.tick_publish_engine_for_now();
        if !retry_frames.is_empty() {
            send_all_outbound(lc.relay_runtime, lc.pool, lc.kernel, retry_frames);
        }
    }

    // ── 7. Temporary-relay idle sweep ────────────────────────────────────
    if *lc.running {
        sweep_temporary_idle_relays(
            lc.relay_runtime,
            lc.pool,
            lc.kernel,
            Instant::now(),
            TEMPORARY_RELAY_IDLE_GRACE,
        );
    }

    // ── 8. Bounded GC pass ───────────────────────────────────────────────
    // #1069 — fires at most once per `GC_TICK_INTERVAL` (60 s).  Wall-clock
    // gate; `Kernel::run_gc_step` uses the injected kernel clock (D9).
    if *lc.running && lc.last_gc.elapsed() >= GC_TICK_INTERVAL {
        lc.kernel.run_gc_step();
        *lc.last_gc = Instant::now();
    }

    // ── 9. Cache-serve step ───────────────────────────────────────────────
    // ADR-0045 §5 — chunked continuation for store-cache serves.  Runs BEFORE
    // the `flush_due` emit below so served events land in this tick's snapshot.
    if *lc.running && (lc.kernel.has_pending_cache_serves() || lc.kernel.has_cache_serve_wakeups())
    {
        lc.kernel.run_cache_serve_step();
    }

    // ── 10. NIP-42 AUTH sign drain ────────────────────────────────────────
    // V-06 / #960 — drain kernel-emitted AUTH signs through the async signer port.
    crate::actor::auth_sign::drain_pending_auth_signs(
        lc.kernel,
        lc.identity,
        lc.parked_ops,
        &mut crate::actor::auth_sign::RouteCtx {
            running: *lc.running,
            queued_publish_outbound: lc.queued_publish_outbound,
            relay_runtime: lc.relay_runtime,
            pool: lc.pool,
        },
    );

    // ── 11. Parked-op drive ───────────────────────────────────────────────
    // ADR-0050 §D2 — ONE `retain_mut` over ONE `Vec<ParkedOp>`.
    if !lc.parked_ops.is_empty() {
        let parked_drive_now = Instant::now();
        let DrainBatch {
            publish: publish_obligations,
            auth: auth_obligations,
            changed: any_changed,
        } = lc.parked_ops.drive_at(lc.kernel, parked_drive_now);

        // Execute NIP-42 AUTH obligations (re-enter after drain's &mut kernel borrow ended).
        crate::actor::auth_sign::run_auth_obligations(
            lc.kernel,
            auth_obligations,
            &mut crate::actor::auth_sign::RouteCtx {
                running: *lc.running,
                queued_publish_outbound: lc.queued_publish_outbound,
                relay_runtime: lc.relay_runtime,
                pool: lc.pool,
            },
        );

        // Execute publish obligations handed back by the `Publish` sink.
        for obligation in publish_obligations {
            match obligation {
                PublishObligation::Publish {
                    signed,
                    p_tags,
                    target,
                    correlation_id_override,
                } => {
                    let outbound = lc.kernel.publish_signed_to_with_correlation(
                        &signed,
                        &p_tags,
                        target,
                        correlation_id_override,
                    );
                    route_dispatch_outbound(
                        *lc.running,
                        lc.queued_publish_outbound,
                        lc.relay_runtime,
                        lc.pool,
                        lc.kernel,
                        outbound,
                    );
                }
                PublishObligation::Failed {
                    toast,
                    correlation_id_override,
                    reason_code,
                } => {
                    lc.kernel.set_last_error_toast(Some(toast.clone()));
                    if let Some(id) = correlation_id_override {
                        lc.kernel
                            .record_action_failure_coded(id, toast, reason_code, None);
                    }
                }
            }
        }

        // Surface changes immediately rather than waiting for the next periodic flush.
        if any_changed && *lc.running {
            emit_now(lc.kernel, *lc.running, lc.update_tx, lc.last_emit);
        }
    }

    // ── 12. Periodic snapshot flush ───────────────────────────────────────
    // Only emit when state actually changed (D8: zero false-wakeup allocations).
    if flush_due(lc.kernel, *lc.running, *lc.last_emit, *lc.emit_hz) {
        emit_now(lc.kernel, *lc.running, lc.update_tx, lc.last_emit);
    }
}
