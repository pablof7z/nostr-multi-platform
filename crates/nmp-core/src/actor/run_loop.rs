//! Actor runtime entry point — the `run_actor_with_observers` bootstrap and its
//! single-inbox priority main loop.
//!
//! Extracted from `actor/mod.rs` to keep that file within the 500 LOC hard cap
//! (AGENTS.md). `mod.rs` retains the module wiring and the always-compiled
//! `ActorCommand` / observer / transport re-exports; this module owns only the
//! native runtime loop that consumes them.
//!
//! The per-iteration lane helpers (`drain_commands`, `run_idle_work`,
//! `LoopContext`) live in `loop_context.rs`; the relay-event guard lives in
//! `relay_event_guard.rs`. This module stitches them together into the actor's
//! bootstrap sequence and the single blocking `recv_timeout` loop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::builtin_projections;
use super::capability_worker::spawn_capability_worker;
use super::commands::IdentityRuntime;
use super::config::{ActorChannels, ActorConfig, ActorRuntimeSlots};
use super::inbox::{CommandLaneDrain, Inbox, LoopStep, MailScheduler};
use super::loop_context::{drain_commands, run_idle_work, DrainResult, LoopContext};
use super::pending_sign::ParkedSignerOps;
use super::raw_event_forwarder;
use super::relay_event_guard;
use super::relay_mgmt::close_relays;
use super::relay_runtime::RelayRuntime;
use super::tick::{compute_wait, emit_now};
use crate::relay::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};

/// T118 / G3 + T146 — actor entry point that accepts BOTH the lifecycle
/// observer slot and the observed-projection sink slot. The FFI lifecycle
/// callback path and Rust observed-projection path share the SAME
/// `Arc<Mutex<…>>` instances so registrations from outside the actor are
/// visible without crossing the FFI on each event.
///
/// Single-inbox priority design (ADR-0050 §D3a): `inbox_rx` carries both
/// commands and relay events as [`ActorMail`]. Each iteration drains the
/// command lane via `try_recv` first (budgeted, stashing any relay mail seen
/// along the way), then makes the loop's single blocking `recv_timeout` — so a
/// command send wakes a relay-blocked actor instead of waiting out the 250 ms
/// idle cap. Command-lane priority and the [`COMMAND_DRAIN_BUDGET`] fairness
/// budget are preserved exactly; relay events still surface at emit-hz cadence
/// when the command lane is not saturated.
///
/// [`ActorMail`]: super::ActorMail
#[cfg(feature = "native")]
pub fn run_actor_with_observers(
    channels: ActorChannels,
    config: ActorConfig,
    runtime: ActorRuntimeSlots,
) {
    let ActorChannels {
        inbox_rx,
        command_tx_self,
        update_tx,
    } = channels;
    let ActorRuntimeSlots {
        lifecycle_observer,
        event_observers,
        snapshot_projections,
        bunker_handshake,
        signer_state,
        bunker_hook,
        external_signer_hook,
        configured_relays,
        mls_local_nsec,
        active_local_keys,
        capability_callback,
        queue_depth,
        routing_trace,
        active_account,
        event_store,
        pull_cursor_registry,
        external_event_sink_dispatcher: dispatcher_slot,
    } = runtime;
    // Dual-channel design: relay events get their own dedicated channel.
    // No merged SyncSender<ActorMsg>, no forwarder threads, no drops.
    //
    // Phase F: the channel item is now [`PoolEvent`] (push-model surface from
    // `nmp_network::pool`). The `Pool` is constructed eagerly here — it owns
    // every per-URL worker thread and the worker→pool translator thread that
    // rewrites `RelayEvent` into `PoolEvent`. Default `PoolConfig` (production
    // keepalive constants, `RelayRole::Content` default lane) matches the
    // pre-Pool actor behaviour bit-for-bit; per-URL role attribution still
    // flows through `Pool::ensure_open_with_role` from `ensure_relay_worker`.
    // ADR-0050 §D3a — the pool delivers relay events through a
    // `RelayMailSink` that wraps each `PoolEvent` into `ActorMail::Relay` and
    // pushes it onto the SAME inbox `inbox_rx` receives commands on. There is
    // no longer a separate `relay_rx`: relay traffic and commands share one
    // waking channel, so a command send wakes a relay-blocked actor.
    let inbox = Inbox::new(inbox_rx);
    let pool = config.build_pool(command_tx_self.relay_sink());

    // The lane scheduler (ADR-0050 §D3a). It owns the relay backlog so any
    // relay mail stashed while draining the command lane each iteration is
    // replayed in order.
    let mut scheduler = MailScheduler::new();

    // The actor owns the only live kernel. FFI/app configuration was snapped at
    // `nmp_app_start` into `config`; runtime-observable handles stay in
    // `runtime` so registrations and publish-back slots preserve identity.
    let mut kernel =
        config.kernel_with_account_slot(DEFAULT_VISIBLE_LIMIT, Arc::clone(&active_account));
    if let Ok(mut guard) = routing_trace.lock() {
        *guard = Some(kernel.routing_trace());
    }
    if let Ok(mut guard) = event_store.lock() {
        *guard = Some(kernel.event_store_handle());
    }
    // ADR-0058 step 3b — publish the kernel's pull-cursor registry handle so the
    // synchronous FFI `pull_page` path can snapshot a registration. Re-published
    // on `Reset` (see dispatch.rs) the same way the event-store handle is.
    if let Ok(mut guard) = pull_cursor_registry.lock() {
        *guard = Some(kernel.pull_cursor_registry_handle());
    }
    config.apply_to_kernel(&mut kernel);
    // G-S4 — bind the actor command-channel depth counter so it surfaces on
    // the diagnostic snapshot (`Metrics::actor_queue_depth`). `NmpApp::send_cmd`
    // increments it; this loop decrements per dequeued command (both recv
    // sites below). Survives `Reset` the same way the drop counter does —
    // re-bound there so the counter stays visible across a kernel rebuild.
    kernel.set_queue_depth_handle(Arc::clone(&queue_depth));
    // T146 — bind the shared observed-projection sink slot. The kernel calls
    // `notify_event_observers` after every `EventStore::insert` returning
    // `Inserted | Replaced` (see `kernel/ingest/timeline.rs`). Per-app
    // crates receive scoped events only through declared observed projections.
    // Survives `Reset` the same way the drop counter does.
    kernel.set_event_observers_handle(Arc::clone(&event_observers));
    // The ExternalEventSinkDispatcher replaces the raw-event-forwarder +
    // pool-send inline path.  The dispatcher owns a bounded channel + worker
    // thread (off the actor thread).  Policies are set via
    // `register_raw_event_forward_policies_from_factory` below and re-installed
    // after every `Reset`.
    //
    // Instance-identity fix: the dispatcher exists from app construction
    // (zero-arg `new()`), so the FFI layer may already have published an
    // instance into `dispatcher_slot` before this actor thread spawned. Adopt
    // that published instance if present so the actor and any FFI handle share
    // one dispatcher. Only if the slot is empty (non-FFI test harnesses) do we
    // create + publish one.
    let external_event_sink_dispatcher = {
        let existing = dispatcher_slot.lock().ok().and_then(|guard| guard.clone());
        match existing {
            Some(d) => d,
            None => {
                let d = crate::substrate::ExternalEventSinkDispatcher::new();
                if let Ok(mut guard) = dispatcher_slot.lock() {
                    *guard = Some(d.clone());
                }
                d
            }
        }
    };
    // Bind the live Pool and spawn the worker thread. Any frames that arrived
    // before this point are retained on the bounded channel and processed as
    // soon as the worker starts.
    external_event_sink_dispatcher.bind_runtime(pool.clone());
    // Bind the dispatcher to the kernel so `persistence.rs` can dispatch
    // frames from the single all-kinds ingest chokepoint.
    kernel.set_external_event_sink_dispatcher(external_event_sink_dispatcher.clone());
    // Raw signed-event forwarding policies are installed through a
    // substrate factory.  The actor contributes only the live kernel
    // handles; target selection and dedup live in the injected policy crate.
    raw_event_forwarder::register_raw_event_forward_policies_from_factory(
        &kernel,
        &external_event_sink_dispatcher,
        config.external_event_sink_policy.clone(),
    );
    // Bind the shared snapshot-projection slot. The kernel runs every
    // host-registered projection closure in `make_update` and appends the
    // result to `KernelSnapshot::projections`. Per-app crates register
    // through the C-ABI `nmp_app_register_snapshot_projection`, which mutates
    // the same `Arc<Mutex<…>>`. Survives `Reset` the same way the other
    // shared handles do so host projections stay live across a kernel
    // rebuild.
    kernel.set_snapshot_projection_handle(Arc::clone(&snapshot_projections));
    builtin_projections::register_builtin_projections(
        &snapshot_projections,
        &bunker_handshake,
        &signer_state,
    );
    // Bind the shared relay-edit rows handle so external Rust callers
    // (e.g. a per-app dispatch crate) can read the user's current
    // relay list without crossing FFI. Survives `Reset` the same way as
    // the other shared handles.
    kernel.set_app_relay_slot(Arc::clone(&configured_relays));
    // D4: the identity runtime is the sole writer of the shared
    // bunker-handshake slot. The built-in `"bunker_handshake"` snapshot
    // projection registered above reads the same `Arc<Mutex<…>>` clone on
    // every tick. Same for `signer_state` (ADR-0048 D6).
    let mut identity = IdentityRuntime::new(bunker_handshake, signer_state);
    // ADR-0052 §D3 — bind the per-app signer hook slots so the FFI broker /
    // NIP-55 driver install into the SAME slots this runtime reads.
    identity.set_signer_hook_slots(bunker_hook, external_signer_hook);
    // V-38: the wallet runtime moved to `nmp-nip47`. The actor no longer
    // owns it; the substrate relay-text interceptor slot
    // (`relay_text_interceptor`) is the only seam the actor calls for NIP-47
    // NWC behavior.
    let mut running = false;
    let mut emit_hz = DEFAULT_EMIT_HZ;
    let mut last_emit = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    // D1 / offline-first §3 — emit one empty-but-valid snapshot from the real
    // kernel before the actor processes any command. Because the same kernel
    // later handles `Start`, a snapshot-first host sees strict rev monotonicity
    // naturally: the initial `running=false` frame is rev=1 and the first
    // `running=true` Start frame is rev=2.
    emit_now(&mut kernel, running, &update_tx, &mut last_emit);

    // #1938 — URL-keyed relay runtime owner. Consolidates the per-URL
    // transport bookkeeping that used to live as five scattered loop-locals:
    // `relay_controls` (one socket per resolved relay URL, keyed by
    // `CanonicalRelayUrl` so the canonicalization invariant is
    // compiler-enforced), `slot_to_url` (reverse lookup from a
    // `RelayHandle.slot()` back to the canonical pool key — handle-carrying
    // `PoolEvent`s don't all carry the URL), `connected_urls` (THE canonical
    // per-socket readiness fact + T116/G1 reconnect-replay discriminator), and
    // `next_relay_generation` (belt-and-braces stale-event stamp). Role
    // readiness is derived from `connected_urls`, not a parallel role-set, so a
    // single sibling-URL failure no longer drops a whole role lane.
    let mut relay_runtime = RelayRuntime::new();
    // #1069 — wall-clock gate for the bounded GC pass. Initialised to "now" so
    // the first pass fires one `GC_TICK_INTERVAL` after the actor starts, not
    // on the cold-start burst (the store is empty then anyway). An `Instant`
    // (performance-timing) read, never the business clock — D9-clean.
    let mut last_gc = Instant::now();
    let mut startup_sent = false;
    // The single unified parked-op queue (ADR-0050 §D2; #1753). `dispatch_command`
    // pushes a `ParkedOp` whenever a remote (NIP-46 / NIP-55) signer goes
    // `Pending` — publish, sign-and-return, the generic sign port, and the
    // cipher port (§D1) all land here and are drained in ONE `drive` below.
    // `ParkedSignerOps` is the target-agnostic queue + drain driver shared with
    // the wasm `KernelReducer` (#1753) so there is one drain, not a parallel copy.
    // Lives outside the loop so parked ops survive across ticks.
    let mut parked_ops = ParkedSignerOps::new();
    let mut queued_publish_outbound = Vec::new();
    let mut first_command = None;

    // ADR-0040 §3 — spawn the serialized capability-worker thread (V-90 Site 2).
    // The worker owns the Receiver; the actor holds `capability_work_tx` and
    // hands borrows of it to `ActorContext` on each dispatch. Dropping
    // `capability_work_tx` on actor teardown closes the channel and the worker
    // exits its blocking `recv` loop cleanly (D8).
    let capability_work_tx =
        spawn_capability_worker(Arc::clone(&capability_callback), command_tx_self.clone());

    loop {
        // ── Priority lane: commands ──────────────────────────────────────
        // Drain a bounded burst of pending commands before touching relay
        // events. Commands still get first service on every iteration, but the
        // budget prevents a sustained command stream from starving relay
        // events, subscription ticks, publish retries, and parked sign ops.
        // Single drain (issue #1231 follow-up #3): `MailScheduler::
        // drain_command_lane` is now the *only* implementation of the
        // command-priority + fairness + relay-backlog contract.
        //
        // `CommandLaneDrain` is `pub(super)` relative to `actor` so we
        // destructure it here (in `mod.rs`, which inhabits `actor`) and pass
        // the data to `drain_commands` in `loop_context.rs`.
        let lane_drain = scheduler.drain_command_lane(&inbox, first_command.take());
        let budget_hit = lane_drain.hit_budget();
        let CommandLaneDrain {
            commands,
            drain: _command_drain,
            disconnected: inbox_disconnected,
        } = lane_drain;
        // `drain_commands` dispatches every command and handles inbox-disconnect
        // shutdown.  On `Shutdown` the relay pool has already been closed; we
        // return immediately.  `budget_hit` (returned on `Continue`) is used
        // below to shorten the relay-lane blocking wait to zero.
        //
        // `LoopContext` is constructed fresh per call — it is a borrowed bundle
        // of `&mut` references into the loop locals; the borrow ends when
        // `drain_commands` returns so the locals are available again in the
        // relay-event and idle-work sections below.
        let command_budget_hit = {
            let mut lc = LoopContext {
                kernel: &mut kernel,
                identity: &mut identity,
                relay_runtime: &mut relay_runtime,
                pool: &pool,
                update_tx: &update_tx,
                last_emit: &mut last_emit,
                last_gc: &mut last_gc,
                running: &mut running,
                emit_hz: &mut emit_hz,
                startup_sent: &mut startup_sent,
                lifecycle_observer: &lifecycle_observer,
                mls_local_nsec: &mls_local_nsec,
                active_local_keys: &active_local_keys,
                capability_callback: &capability_callback,
                parked_ops: &mut parked_ops,
                queued_publish_outbound: &mut queued_publish_outbound,
                command_tx_self: &command_tx_self,
                capability_work_tx: &capability_work_tx,
                config: &config,
                routing_trace_slot: &routing_trace,
                event_store_slot: &event_store,
                pull_cursor_registry_slot: &pull_cursor_registry,
                active_account_slot: &active_account,
                external_event_sink_dispatcher: &external_event_sink_dispatcher,
                queue_depth: &queue_depth,
            };
            match drain_commands(&mut lc, commands, inbox_disconnected, budget_hit) {
                DrainResult::Shutdown => return,
                DrainResult::Continue { budget_hit } => budget_hit,
            }
        };

        // ── Relay event lane ─────────────────────────────────────────────
        // Block up to compute_wait so emit-hz is respected without busy-spin.
        // This `recv_timeout` is the loop's SINGLE blocking point (D8): a
        // backlog relay event (stashed while draining commands) is served
        // first with zero wait; otherwise we block on the unified inbox, so a
        // command send wakes us here too. A command
        // received during the wait is replayed as `first_command` so the next
        // iteration dispatches it on the priority lane (no added latency).
        //
        // Phase F: the inbound item is `PoolEvent` (push-model). Stale-event
        // filtering moved into `handle_relay_event` itself — the helper
        // resolves `RelayHandle.slot()` → `(url, role)` via the
        // `slot_to_url` side-map and the `relay_controls` entry, dropping
        // any handle whose generation no longer matches the slot's current
        // generation. The pool's translator already drops events with a
        // stale slot-generation, so this is belt-and-braces.
        // Relay events are processed under panic isolation — see
        // `relay_event_guard::process_relay_event`. `handle_relay_event`
        // parses arbitrary network bytes (the highest-risk panic site in the
        // actor); the guard's `catch_unwind` keeps a panic from killing the
        // loop (D1: partial state tolerated, loop survival is the invariant).
        // The same guarded helper serves BOTH the bounded backlog batch and
        // the single recv'd event below (#1264).
        //
        // A small local macro forwards the actor's ~13 loop locals into the
        // helper from both call sites without re-listing them (a closure would
        // have to mutably re-borrow them per batch element).
        macro_rules! process_relay_event {
            ($event:expr) => {
                relay_event_guard::process_relay_event(
                    $event,
                    &mut kernel,
                    &config.relay_text_interceptors,
                    &config.relay_connected_hooks,
                    &command_tx_self,
                    &mut relay_runtime,
                    &pool,
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

        // ── Relay event lane ─────────────────────────────────────────────
        // Block up to compute_wait so emit-hz is respected without busy-spin.
        // This `recv_timeout` is the loop's SINGLE blocking point (D8). A
        // command received during the wait is replayed as `first_command` so
        // the next iteration dispatches it on the priority lane (no added
        // latency).
        //
        // #1264: when backlog work remains (the batch did not exhaust it) we
        // pass a ZERO wait so the loop keeps draining promptly — but we STILL
        // call `recv_timeout`, so the single blocking point is reached every
        // iteration (no busy-spin / no D8 violation: a zero-timeout `recv` is
        // the one wait, it simply returns immediately when nothing is queued).
        //
        // Phase F: the inbound item is `PoolEvent` (push-model). Stale-event
        // filtering moved into `handle_relay_event` itself — the helper
        // resolves `RelayHandle.slot()` → `(url, role)` via the `slot_to_url`
        // side-map and the `relay_controls` entry, dropping any handle whose
        // generation no longer matches the slot's current generation. The
        // pool's translator already drops events with a stale slot-generation,
        // so this is belt-and-braces.
        // When the command-drain budget was hit, use a zero relay-lane wait so
        // a command burst is not stalled by the full idle cap.  Mirrors the
        // former `command_drain.relay_wait(computed_wait)` call: that helper
        // returned `Duration::ZERO` when `hit_budget()` was true.
        let wait = if scheduler.has_backlog() || command_budget_hit {
            std::time::Duration::ZERO
        } else {
            compute_wait(&kernel, running, last_emit, emit_hz)
        };
        match scheduler.next_after_drain(&inbox, wait) {
            LoopStep::Command(command) => {
                // Woken by a command during the blocking wait — replay it on
                // next iteration's priority lane (zero added latency).
                first_command = Some(command);
            }
            LoopStep::Shutdown => {
                close_relays(&mut relay_runtime, &pool, &mut kernel);
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
        // All idle-tick maintenance is delegated to `run_idle_work` in
        // `loop_context.rs` (extracted to keep `mod.rs` within its LOC budget).
        // See that function's doc-comment for the ordered sub-step list.
        // The `LoopContext` borrow ends when `run_idle_work` returns, so the
        // loop-locals are free again for the next iteration.
        {
            let mut lc = LoopContext {
                kernel: &mut kernel,
                identity: &mut identity,
                relay_runtime: &mut relay_runtime,
                pool: &pool,
                update_tx: &update_tx,
                last_emit: &mut last_emit,
                last_gc: &mut last_gc,
                running: &mut running,
                emit_hz: &mut emit_hz,
                startup_sent: &mut startup_sent,
                lifecycle_observer: &lifecycle_observer,
                mls_local_nsec: &mls_local_nsec,
                active_local_keys: &active_local_keys,
                capability_callback: &capability_callback,
                parked_ops: &mut parked_ops,
                queued_publish_outbound: &mut queued_publish_outbound,
                command_tx_self: &command_tx_self,
                capability_work_tx: &capability_work_tx,
                config: &config,
                routing_trace_slot: &routing_trace,
                event_store_slot: &event_store,
                pull_cursor_registry_slot: &pull_cursor_registry,
                active_account_slot: &active_account,
                external_event_sink_dispatcher: &external_event_sink_dispatcher,
                queue_depth: &queue_depth,
            };
            run_idle_work(&mut lc);
        }
    }
}
