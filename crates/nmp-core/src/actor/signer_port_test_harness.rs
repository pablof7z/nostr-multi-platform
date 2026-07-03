//! Shared test harness for the ADR-0072 signer-port dispatch tests.
//!
//! [`dispatch_one`] builds a fully-wired [`ActorContext`] and runs a single
//! `dispatch_command(cmd, ctx)`, returning the parked-op queue so the sign /
//! cipher port tests can resolve + drain. Extracted from the two
//! `*_for_account_tests.rs` files (which each carried an identical copy) so they
//! stay within the file-size ceiling.

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use super::commands::{self, IdentityRuntime};
use super::dispatch::{dispatch_command, ActorContext};
use super::pending_sign::{ParkedOp, ParkedSignerOps};
use super::{ActorCommand, ActorConfigSources, ActorMail, CommandSender};
use crate::kernel::Kernel;

/// Drive a single `dispatch_command(cmd, ctx)` against a freshly built
/// [`ActorContext`], returning the unified parked-op queue afterwards.
pub(super) fn dispatch_one(
    cmd: ActorCommand,
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
) -> Vec<ParkedOp> {
    let pool = nmp_network::pool::Pool::new(
        nmp_network::pool::PoolConfig::default(),
        channel::<nmp_network::pool::PoolEvent>().0,
    );
    let mut relay_runtime = super::relay_runtime::RelayRuntime::new();
    dispatch_one_with_relays(cmd, identity, kernel, &pool, &mut relay_runtime, true)
}

/// Lower-level variant of [`dispatch_one`] that threads caller-owned relay
/// transport state (pool + control maps + generation counter) through the
/// [`ActorContext`] so a test can pre-seed relay workers and inspect them after
/// the dispatch. `running` selects the actor's running flag (the fail-closed
/// gate the `ReconnectRelays` arm checks). Returns the parked-op queue.
pub(super) fn dispatch_one_with_relays(
    cmd: ActorCommand,
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    pool: &nmp_network::pool::Pool,
    relay_runtime: &mut super::relay_runtime::RelayRuntime,
    running_flag: bool,
) -> Vec<ParkedOp> {
    use std::time::Instant;

    let (update_tx, _update_rx) = channel::<crate::update_envelope::UpdateFrameBytes>();
    let (command_inbox_tx, _command_rx) = channel::<ActorMail>();
    let command_tx = CommandSender::new(command_inbox_tx);
    let lifecycle_observer = commands::new_observer_slot();
    let mls_local_nsec = Arc::new(Mutex::new(None));
    let active_local_keys = Arc::new(Mutex::new(None));
    let mut last_emit = Instant::now();
    let mut running = running_flag;
    let mut emit_hz = 4u32;
    let mut startup_sent = false;
    let mut parked_ops = ParkedSignerOps::new();
    let capability_callback = crate::capability_socket::new_capability_callback_slot();
    let (capability_work_inner_tx, _capability_work_rx) = channel::<ActorMail>();
    let capability_work_tx = crate::actor::capability_worker::spawn_capability_worker(
        Arc::clone(&capability_callback),
        CommandSender::new(capability_work_inner_tx),
    );
    let coverage_hook = Arc::new(Mutex::new(None::<crate::subs::PlanCoverageHook>));
    let req_frame_interceptor = Arc::new(Mutex::new(None));
    let host_op_handler = Arc::new(Mutex::new(None));
    let ingest_dispatcher_slot = Arc::new(std::sync::RwLock::new(
        crate::substrate::EventIngestDispatcher::default(),
    ));
    let dm_inbox_relays_slot =
        Arc::new(Mutex::new(crate::substrate::empty_dm_inbox_relay_lookup()));
    let profile_lookup_slot = Arc::new(Mutex::new(crate::substrate::empty_profile_lookup()));
    let blocked_relays_slot = Arc::new(Mutex::new(crate::substrate::empty_blocked_relay_lookup()));
    let bootstrap_self_kinds_slot = Arc::new(Mutex::new(None));
    let routing_trace_slot = Arc::new(Mutex::new(None));
    let event_store_slot = Arc::new(Mutex::new(None));
    let pull_cursor_registry_slot = crate::slots::new_pull_cursor_registry_handle_slot();
    let active_account_slot = crate::slots::new_active_account_slot();
    let external_event_sink_dispatcher = crate::substrate::ExternalEventSinkDispatcher::new();
    let config = ActorConfigSources {
        storage_path: Arc::new(Mutex::new(None)),
        coverage_hook,
        req_frame_interceptor,
        host_op_handler,
        relay_text_interceptor: crate::substrate::new_relay_text_interceptor_slot(),
        relay_connected_hook: crate::substrate::new_relay_connected_hook_slot(),
        ingest_dispatcher: ingest_dispatcher_slot,
        search_scope_registry: Arc::new(crate::substrate::SearchScopeRegistry::new()),
        draft_builders: Arc::new(crate::substrate::DraftBuilderRegistry::new()),
        dm_inbox_relays: dm_inbox_relays_slot,
        contact_list_reader: crate::slots::new_contact_list_reader_slot(),
        profile_lookup: profile_lookup_slot,
        blocked_relays: blocked_relays_slot,
        bootstrap_self_kinds: bootstrap_self_kinds_slot,
        routing_substrate: crate::slots::new_routing_substrate_slot(),
        publish_resolver: crate::slots::new_publish_resolver_slot(),
        relay_list_publish_support: crate::slots::new_relay_list_publish_support_slot(),
        external_event_sink_policy: crate::slots::new_external_event_sink_policy_slot(),
        kernel_clock: crate::slots::new_kernel_clock_slot(),
        gc_budget_ceiling: None,
        user_agent: Arc::new(Mutex::new(None)),
        outbound_public_tags: Arc::new(Mutex::new(None)),
    }
    .snapshot();

    let mut ctx = ActorContext {
        kernel,
        identity,
        relay_runtime,
        pool,
        update_tx: &update_tx,
        last_emit: &mut last_emit,
        dispatch_now: Instant::now(),
        running: &mut running,
        emit_hz: &mut emit_hz,
        startup_sent: &mut startup_sent,
        relays_ready: false,
        lifecycle_observer: &lifecycle_observer,
        mls_local_nsec: &mls_local_nsec,
        active_local_keys: &active_local_keys,
        capability_callback: &capability_callback,
        parked_ops: &mut parked_ops,
        command_tx_self: &command_tx,
        capability_work_tx: &capability_work_tx,
        config: &config,
        routing_trace_slot: &routing_trace_slot,
        event_store_slot: &event_store_slot,
        pull_cursor_registry_slot: &pull_cursor_registry_slot,
        active_account_slot: &active_account_slot,
        external_event_sink_dispatcher: &external_event_sink_dispatcher,
    };
    dispatch_command(cmd, &mut ctx);
    parked_ops.into_vec()
}
