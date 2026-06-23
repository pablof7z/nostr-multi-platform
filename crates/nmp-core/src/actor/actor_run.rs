//! Native actor entry point — `run_actor_with_observers`.
//!
//! Extracted from `actor/mod.rs` to keep that file within the 500-LOC ceiling
//! (AGENTS.md §file-size). Zero logic changes — pure structural split.
//!
//! This module is only compiled when `feature = "native"` (via the gated
//! `mod actor_run;` declaration in `actor/mod.rs`).

use super::commands::IdentityRuntime;
use super::{ActorChannels, ActorConfig, ActorRuntimeSlots, GC_TICK_INTERVAL, RelayControl};
use super::{auth_sign, raw_event_forwarder, relay_event_guard, typed_projections};
use super::capability_worker::spawn_capability_worker;
use super::dispatch::{dispatch_command, ActorContext};
use super::inbox::{CommandLaneDrain, Inbox, LoopStep, MailScheduler};
use super::outbound::wire_frames_to_outbound;
use super::pending_sign::{self, ParkedSignerOps, PublishObligation};
use super::relay_idle::{sweep_temporary_idle_relays, TEMPORARY_RELAY_IDLE_GRACE};
use super::relay_mgmt::{
    claim_send_gate, close_relays, maybe_send_startup, route_dispatch_outbound, send_all_outbound,
};
use super::tick::{compute_wait, emit_now, flush_due};
use crate::relay::{CanonicalRelayUrl, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use nmp_network::pool::{Pool, PoolConfig};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::actor_loop::{run_actor_loop, ActorLoopState};

/// T118 / G3 + T146 — actor entry point that accepts BOTH the lifecycle
/// observer slot and the kernel event observer slot. The FFI
/// (`ffi/lifecycle.rs::nmp_app_set_lifecycle_callback`,
/// `ffi/event_observer.rs::nmp_app_register_event_observer`) shares the SAME
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
    let pool = Pool::new(PoolConfig::default(), command_tx_self.relay_sink());

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
    // T146 — bind the shared kernel event observer slot. The kernel calls
    // `notify_event_observers` after every `EventStore::insert` returning
    // `Inserted | Replaced` (see `kernel/ingest/timeline.rs`). Per-app
    // crates (e.g. `nmp-app-chirp`) clone this slot via
    // `NmpApp::register_event_observer` to register typed observers.
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
    // D0 — register the built-in `"bunker_handshake"` snapshot projection.
    // NIP-46 remote signing is an app noun, so handshake state is NOT a typed
    // `KernelSnapshot` field — it is projected under
    // `projections["bunker_handshake"]` exactly like a host-registered
    // namespace. The closure reads the shared bunker-handshake slot the
    // actor's `IdentityRuntime` writes; it runs on every snapshot tick (D8:
    // cheap, non-blocking — a single lock-and-clone). When no handshake is in
    // flight the slot holds `None` and the closure contributes JSON `null`,
    // preserving the "key present, value null when idle" semantic the host
    // sign-in flow decodes. Registered here (the actor wiring site) rather than
    // on the FFI surface so every actor consumer — FFI or test — gets it.
    {
        // Typed sidecar (ADR-0037).
        let typed_slot = Arc::clone(&bunker_handshake);
        if let Ok(mut registry) = snapshot_projections.lock() {
            registry.register_typed("bunker_handshake", move || {
                typed_projections::bunker_handshake_typed(&typed_slot)
            });
        }
    }
    // D0 — second built-in NIP-46 projection: `"nip46_onboarding"`. Where
    // `"bunker_handshake"` carries the raw broker progress (stage string +
    // message), this projection carries the *typed* onboarding read model
    // shells render directly — the static signer-app probe table, the typed
    // `stage_kind`, and pre-computed `is_in_flight` / `is_failed` /
    // `is_terminal_success` / `can_cancel` flags. The closure reads the same
    // shared bunker-handshake slot the previous projection serializes, plus a
    // Rust-owned static signer-app list (no platform-shell ownership of
    // protocol-knowledge tables). Always present (never JSON null) so the host
    // can read `signer_apps` even when no handshake is in flight.
    //
    // ADR-0055 R6-S2: the typed sidecar now uses `TypedProjectionEmissionState`
    // to omit an unchanged `nip46_onboarding` frame when the host has declared
    // incremental-apply capability (exact byte equality, monotonic rev, freeze
    // guard on the same FrameIdentity the feed uses — one implementation, shared).
    {
        // Typed sidecar (ADR-0037).
        let typed_slot = Arc::clone(&bunker_handshake);
        // R6-S2: read the frame-identity and capability handles from the
        // registry (one lock) before the registration lock below.
        let (
            nip46_onboarding_incremental_apply,
            nip46_onboarding_frame_session_id,
            nip46_onboarding_frame_snapshot_epoch,
        ) = if let Ok(reg) = snapshot_projections.lock() {
            let cap = reg.incremental_apply_handle();
            let (sid, epoch) = reg.frame_identity_handles();
            (cap, sid, epoch)
        } else {
            use std::sync::atomic::AtomicU64;
            (
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
            )
        };
        let nip46_onboarding_emission_state = Arc::new(Mutex::new(
            crate::projection_emission::TypedProjectionEmissionState::new(
                nip46_onboarding_incremental_apply,
            ),
        ));
        if let Ok(mut registry) = snapshot_projections.lock() {
            let emission_state = Arc::clone(&nip46_onboarding_emission_state);
            let frame_session_id = Arc::clone(&nip46_onboarding_frame_session_id);
            let frame_snapshot_epoch = Arc::clone(&nip46_onboarding_frame_snapshot_epoch);
            registry.register_typed("nip46_onboarding", move || {
                // Build the typed payload (always Some for nip46_onboarding).
                let typed_data = typed_projections::nip46_onboarding_typed(&typed_slot)?;
                // R6-S2: apply byte-equality omit (same mechanism as feed R6-S1).
                let identity = crate::projection_emission::FrameIdentity {
                    session_id: frame_session_id.load(Ordering::Acquire),
                    snapshot_epoch: frame_snapshot_epoch.load(Ordering::Acquire),
                };
                let Ok(mut state) = emission_state.lock() else {
                    // Poisoned mutex — degrade to always-emit (D6: safe fallback).
                    return Some(typed_data);
                };
                let emit_decision = state.should_emit(typed_data.payload.clone(), identity);
                drop(state);
                match emit_decision {
                    None => None,
                    Some((payload, projection_rev)) => {
                        Some(crate::update_envelope::TypedProjectionData {
                            payload,
                            projection_rev,
                            ..typed_data
                        })
                    }
                }
            });
        }
    }
    // ADR-0048 D6 — generalised remote-signer health projection: `"signer_state"`.
    // Replaces the NIP-46-only `"bunker_connection_state"` (V-14 step b) with a
    // unified surface keyed by `signer_kind` (`"nip46"` | `"nip55"`). Both
    // signers write into the same slot via `IdentityRuntime::set_signer_state`.
    // `None` (no active remote signer session) → JSON `null`.
    // D0: remote-signer health is an app noun, not a typed `KernelSnapshot` field.
    {
        // Typed sidecar (ADR-0037).
        let typed_slot = Arc::clone(&signer_state);
        if let Ok(mut registry) = snapshot_projections.lock() {
            registry.register_typed("signer_state", move || {
                typed_projections::signer_state_typed(&typed_slot)
            });
        }
    }
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
    let running = false;
    let emit_hz = DEFAULT_EMIT_HZ;
    // Initialise to "1 s ago" so the first idle tick can emit immediately.
    let mut last_emit = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    // D1 / offline-first §3 — emit one empty-but-valid snapshot from the real
    // kernel before the actor processes any command. Because the same kernel
    // later handles `Start`, a snapshot-first host sees strict rev monotonicity
    // naturally: the initial `running=false` frame is rev=1 and the first
    // `running=true` Start frame is rev=2.
    emit_now(&mut kernel, running, &update_tx, &mut last_emit);

    // T105: URL-keyed transport pool. One socket per resolved relay URL;
    // workers spawn on demand as OutboundMessages flow with new relay_urls.
    // Keyed by `CanonicalRelayUrl` so the canonicalization invariant is
    // compiler-enforced — a raw `&str` cannot index the pool.
    let relay_controls: HashMap<CanonicalRelayUrl, RelayControl> = HashMap::new();
    // Phase F: reverse lookup from a `RelayHandle.slot()` back to the
    // canonical pool key. Inbound `PoolEvent`s carry the handle but not the
    // URL on every variant (`Opened` carries it; `Frame`/`Closed`/`Failed`
    // do not), so we maintain this side-map alongside `relay_controls` so
    // the event dispatcher can resolve `slot → (url, role)` without an
    // O(n) scan. Inserted by `ensure_relay_worker`, removed by
    // `shutdown_relay_worker` / `close_relays`.
    let slot_to_url: HashMap<u32, CanonicalRelayUrl> = HashMap::new();
    let connected_relays = HashSet::new();
    let connected_urls: HashSet<CanonicalRelayUrl> = HashSet::new(); // T116/G1 reconnect-replay discriminator.
    let next_relay_generation = 1u64;
    // #1069 — wall-clock gate for the bounded GC pass. Initialised to "now" so
    // the first pass fires one `GC_TICK_INTERVAL` after the actor starts, not
    // on the cold-start burst (the store is empty then anyway). An `Instant`
    // (performance-timing) read, never the business clock — D9-clean.
    let last_gc = Instant::now();
    let startup_sent = false;
    // The single unified parked-op queue (ADR-0050 §D2; #1753). `dispatch_command`
    // pushes a `ParkedOp` whenever a remote (NIP-46 / NIP-55) signer goes
    // `Pending` — publish, sign-and-return, the generic sign port, and the
    // cipher port (§D1) all land here and are drained in ONE `drive` below.
    // `ParkedSignerOps` is the target-agnostic queue + drain driver shared with
    // the wasm `KernelReducer` (#1753) so there is one drain, not a parallel copy.
    // Lives outside the loop so parked ops survive across ticks.
    let parked_ops = ParkedSignerOps::new();
    let queued_publish_outbound = Vec::new();
    let first_command = None;

    // ADR-0040 §3 — spawn the serialized capability-worker thread (V-90 Site 2).
    // The worker owns the Receiver; the actor holds `capability_work_tx` and
    // hands borrows of it to `ActorContext` on each dispatch. Dropping
    // `capability_work_tx` on actor teardown closes the channel and the worker
    // exits its blocking `recv` loop cleanly (D8).
    let capability_work_tx =
        spawn_capability_worker(Arc::clone(&capability_callback), command_tx_self.clone());

    // Bundle loop-local state and hand off to the extracted main loop.
    let loop_state = ActorLoopState {
        running,
        emit_hz,
        last_emit,
        relay_controls,
        slot_to_url,
        connected_relays,
        connected_urls,
        next_relay_generation,
        last_gc,
        startup_sent,
        parked_ops,
        queued_publish_outbound,
        first_command,
    };

    run_actor_loop(
        loop_state,
        kernel,
        inbox,
        scheduler,
        pool,
        update_tx,
        queue_depth,
        config,
        &mut identity,
        &lifecycle_observer,
        &mls_local_nsec,
        &active_local_keys,
        &capability_callback,
        &command_tx_self,
        &capability_work_tx,
        &routing_trace,
        &event_store,
        &pull_cursor_registry,
        &active_account,
        &external_event_sink_dispatcher,
    );
}
