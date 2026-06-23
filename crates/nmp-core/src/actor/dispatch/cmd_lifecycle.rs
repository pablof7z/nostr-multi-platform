//! Lifecycle command dispatch arms: `Start`, `Stop`, `Reset`, `Shutdown` +
//! `LifecycleEvent` + `MarkChangedSinceEmit`.
//!
//! Extracted from `dispatch/mod.rs` to keep it under the 500-LOC ceiling.
//! No behaviour change — all logic is verbatim from the original file.
//!
//! ADR-0065 — the `dispatch` function below matches the `LifecycleCommand`
//! sub-enum and routes each verb to its existing handler.

use std::sync::Arc;

use crate::actor::relay_mgmt::{close_relays, spawn_missing_relays};
use crate::actor::session_persistence;
use crate::actor::tick::{clamp_emit_hz_logged, emit_now, maybe_emit_after_dispatch};
use crate::kernel::LifecyclePhase;
use crate::relay::OutboundMessage;

use super::helpers::update_local_key_slots;
use super::{commands, ActorContext, LifecycleCommand};

/// Dispatch `ActorCommand::Start`.
pub(super) fn start(
    visible_limit: usize,
    requested_hz: u32,
    initial_relays: Vec<(String, String)>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    use crate::actor::tick::clamp_emit_hz_logged;
    *ctx.running = true;
    *ctx.emit_hz = clamp_emit_hz_logged(ctx.kernel, requested_hz, "Start"); // D8 ceiling
    *ctx.startup_sent = false;
    ctx.kernel.set_visible_limit(visible_limit);
    // Seed the app-declared initial relay configuration into
    // `configured_relays` before the session restore runs. There is no
    // hardcoded default: an app with no declared relays (and no pre-start
    // `add_relay`) starts with an empty set and the kernel surfaces the
    // `no_configured_relays` diagnostic (V-66) rather than silently
    // dialing an unconsented relay.
    if !initial_relays.is_empty() {
        let rows: Vec<crate::kernel::AppRelay> = initial_relays
            .iter()
            .filter_map(|(url, role)| {
                let url = crate::relay::canonical_relay_url(url)?;
                let role = crate::actor::canonical_relay_role(role)?;
                Some(crate::kernel::AppRelay::new(url, role))
            })
            .collect();
        if !rows.is_empty() {
            ctx.kernel.set_configured_relays(rows);
        }
    }
    ctx.kernel.start();
    // ADR-0040 §3: restore_active_session stays synchronous (cold-start
    // read chain; see session_persistence.rs module doc). The tail
    // writes (persist_current_active_session) are enqueued off-actor.
    let mut outbound = session_persistence::restore_active_session(
        ctx.identity,
        ctx.kernel,
        ctx.capability_callback,
        ctx.capability_work_tx,
        ctx.relays_ready,
    );
    update_local_key_slots(ctx.identity, ctx.mls_local_nsec, ctx.active_local_keys);
    // D1 — first snapshot must reach the shell before any relay TCP
    // connection is dialed, so emit_now precedes spawn_missing_relays.
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    spawn_missing_relays(
        ctx.relay_controls,
        ctx.slot_to_url,
        ctx.pool,
        ctx.kernel,
        ctx.next_relay_generation,
    );
    // T127: boot-resume for the publish engine. Closes Residual 3
    // from T117 — `accepted_locally` rows persisted by a previous
    // process come back as `InFlight` and any due retries dispatch
    // immediately. Today the production store is fresh in-memory
    // per process so this is a no-op; once the M3 LMDB store lands
    // the resume call will drive the resurrected rows back through
    // the actor's normal outbound path. `spawn_missing_relays`
    // above ran first, so workers will spawn on demand for any
    // URL the resumed frames target (idempotent via
    // `ensure_relay_worker`). Frames flow through the regular
    // `send_all_outbound` call in `run_actor`.
    outbound.extend(ctx.kernel.resume_publish_engine());
    Some(outbound)
}

/// Dispatch `ActorCommand::Stop`.
pub(super) fn stop(ctx: &mut ActorContext<'_>) -> Option<Vec<OutboundMessage>> {
    *ctx.running = false;
    *ctx.startup_sent = false;
    close_relays(
        ctx.relay_controls,
        ctx.slot_to_url,
        ctx.pool,
        ctx.connected_relays,
        ctx.kernel,
    );
    // T116/G1 — clear reconnect-replay discriminator so a subsequent
    // Start replays cleanly (every URL appears as a first-connect).
    ctx.connected_urls.clear();
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(Vec::new())
}

/// Dispatch `ActorCommand::Shutdown` — signals the actor loop to exit.
pub(super) fn shutdown(ctx: &mut ActorContext<'_>) -> Option<Vec<OutboundMessage>> {
    close_relays(
        ctx.relay_controls,
        ctx.slot_to_url,
        ctx.pool,
        ctx.connected_relays,
        ctx.kernel,
    );
    ctx.connected_urls.clear();
    None
}

/// Dispatch `ActorCommand::Reset`.
///
/// Wipes the kernel state, preserves shared `Arc` handles so the host's
/// FFI surface keeps working across the state wipe, and re-starts relay
/// workers if the actor is currently running.
pub(super) fn reset(ctx: &mut ActorContext<'_>) -> Option<Vec<OutboundMessage>> {
    close_relays(
        ctx.relay_controls,
        ctx.slot_to_url,
        ctx.pool,
        ctx.connected_relays,
        ctx.kernel,
    );
    ctx.connected_urls.clear();
    // G-S4 — preserve the actor command-channel depth counter across
    // Reset for the same reason: the `Arc<AtomicU64>` is shared with
    // `NmpApp::send_cmd`; replacing it would orphan the counter so
    // every subsequent send increments into a handle the kernel no
    // longer reads.
    let queue_depth_handle = ctx.kernel.take_queue_depth_handle_for_reset();
    // T146 — preserve the event observer slot across Reset for the
    // same reason: the `Arc<Mutex<…>>` is shared with the FFI
    // surface and per-app crates; replacing it would silently
    // disconnect every registered observer.
    let event_observers_handle = ctx.kernel.take_event_observers_handle_for_reset();
    // Preserve the snapshot-projection slot across Reset for the same
    // reason: the `Arc<Mutex<…>>` is shared with the FFI surface and
    // per-app crates; replacing it would silently drop every
    // host-registered projection from the snapshot.
    let snapshot_projection_handle = ctx.kernel.take_snapshot_projection_handle_for_reset();
    // Preserve the relay-edit rows handle across Reset for the same
    // reason: the `Arc<Mutex<…>>` is shared with the FFI surface
    // and per-app crates; replacing it would silently return stale
    // rows to the host-app dispatch layer.
    let configured_relays_handle = ctx.kernel.take_app_relay_slot_for_reset();
    // V-82 — rebuild over the SAME FFI-shared active-account slot so
    // `NmpApp::active_account_handle()` keeps reading the slot the
    // rebuilt kernel writes (a bare `Kernel::new` would mint a fresh
    // slot and silently orphan the host's handle on every Reset).
    // Mirrors the routing-trace re-publish contract below: the shared
    // `Arc` outlives the discarded kernel.
    *ctx.kernel = ctx.config.kernel_with_account_slot(
        ctx.kernel.visible_limit(),
        Arc::clone(ctx.active_account_slot),
    );
    // V-82 — clear the shared active-account slot to match the fresh
    // kernel's empty `active_account` projection. The rebuilt kernel
    // only writes the slot on the next identity mutation (`set_accounts`),
    // so without this the slot would retain the pre-Reset pubkey and
    // `NmpApp::active_account_handle()` would report a stale account
    // while every other projection says "no account". Pre-V-82 the
    // host-observable post-Reset value was `None` (the discarded kernel
    // minted a fresh empty slot); clearing here preserves that. D6:
    // poisoned lock → silent no-op, matching the other slots' policy.
    if let Ok(mut guard) = ctx.active_account_slot.lock() {
        *guard = None;
    }
    if let Some(handle) = queue_depth_handle {
        ctx.kernel.set_queue_depth_handle(handle);
    }
    if let Some(handle) = event_observers_handle {
        ctx.kernel.set_event_observers_handle(handle);
    }
    // Re-bind the dispatcher to the new kernel (the dispatcher itself
    // survives Reset — it is Arc-based and its background thread is
    // permanent; only the kernel reference needs updating).
    ctx.kernel
        .set_external_event_sink_dispatcher(ctx.external_event_sink_dispatcher.clone());
    if let Some(handle) = snapshot_projection_handle {
        ctx.kernel.set_snapshot_projection_handle(handle);
    }
    if let Some(handle) = configured_relays_handle {
        ctx.kernel.set_app_relay_slot(handle);
    }
    ctx.config.apply_to_kernel(ctx.kernel);
    // V-51 phase 4 — re-publish the rebuilt kernel's routing-trace
    // projection clone into the shared slot. The previous projection
    // was attached to the now-discarded kernel; `Reset` is a "wipe
    // state" command and the reader contract is "the most recent
    // routing decisions of the live kernel".
    if let Ok(mut guard) = ctx.routing_trace_slot.lock() {
        *guard = Some(ctx.kernel.routing_trace());
    }
    // V-83 — re-publish the rebuilt kernel's `EventStore` handle clone.
    // `Reset` constructed a fresh kernel (and hence a fresh store) above;
    // without this the slot would retain a handle to the discarded
    // kernel's store and `NmpApp::event_by_id` would read stale (empty
    // post-wipe) data. Same publish-back-on-`Reset` contract as the
    // routing-trace projection above.
    if let Ok(mut guard) = ctx.event_store_slot.lock() {
        *guard = Some(ctx.kernel.event_store_handle());
    }
    // ADR-0058 step 3b — re-publish the rebuilt kernel's pull-cursor
    // registry handle (the `Reset` above minted a fresh registry) so the
    // FFI `pull_page` path reads the live kernel's cursors, not the
    // discarded kernel's. Same publish-back-on-`Reset` contract as the
    // event-store handle above.
    if let Ok(mut guard) = ctx.pull_cursor_registry_slot.lock() {
        *guard = Some(ctx.kernel.pull_cursor_registry_handle());
    }
    // Re-register injected raw-event forwarding policies against the
    // rebuilt kernel. The prior observers captured handles from the
    // discarded kernel; re-running the factory preserves policy
    // registrations while keeping target selection out of core.
    crate::actor::raw_event_forwarder::register_raw_event_forward_policies_from_factory(
        ctx.kernel,
        ctx.external_event_sink_dispatcher,
        ctx.config.external_event_sink_policy.clone(),
    );
    *ctx.startup_sent = false;
    if *ctx.running {
        ctx.kernel.start();
        // D1 — first snapshot must reach the shell before any relay TCP
        // connection is dialed, so emit_now precedes spawn_missing_relays.
        emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
        spawn_missing_relays(
            ctx.relay_controls,
            ctx.slot_to_url,
            ctx.pool,
            ctx.kernel,
            ctx.next_relay_generation,
        );
    }
    Some(Vec::new())
}

/// ADR-0065 — `LifecycleCommand` family dispatch. Matches the sub-enum and
/// routes each verb to its existing handler. `Configure` / `LifecycleEvent` /
/// `MarkChangedSinceEmit` are small enough to inline.
pub(super) fn dispatch(
    cmd: LifecycleCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        LifecycleCommand::Start { visible_limit, emit_hz: requested_hz, initial_relays } =>
            start(visible_limit, requested_hz, initial_relays, ctx),
        LifecycleCommand::Configure { visible_limit, emit_hz: requested_hz } => {
            *ctx.emit_hz = clamp_emit_hz_logged(ctx.kernel, requested_hz, "Configure");
            ctx.kernel.set_visible_limit(visible_limit);
            emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        LifecycleCommand::LifecycleEvent(phase) => {
            commands::handle_lifecycle_event(ctx.kernel, ctx.lifecycle_observer, phase);
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        LifecycleCommand::MarkChangedSinceEmit => {
            ctx.kernel.mark_changed_since_emit();
            maybe_emit_after_dispatch(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
            Some(Vec::new())
        }
        LifecycleCommand::Stop => stop(ctx),
        LifecycleCommand::Reset => reset(ctx),
        LifecycleCommand::Shutdown => shutdown(ctx),
    }
}
