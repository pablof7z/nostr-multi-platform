//! Shared test harness for the ADR-0050 signer-port dispatch tests.
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
use super::pending_sign::ParkedOp;
use super::{ActorCommand, ActorMail, CommandSender};
use crate::kernel::Kernel;

/// Drive a single `dispatch_command(cmd, ctx)` against a freshly built
/// [`ActorContext`], returning the unified parked-op queue afterwards.
pub(super) fn dispatch_one(
    cmd: ActorCommand,
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
) -> Vec<ParkedOp> {
    use crate::relay::CanonicalRelayUrl;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    let (update_tx, _update_rx) = channel::<crate::update_envelope::UpdateFrameBytes>();
    let (command_inbox_tx, _command_rx) = channel::<ActorMail>();
    let command_tx = CommandSender::new(command_inbox_tx);
    let lifecycle_observer = commands::new_observer_slot();
    let mls_local_nsec = Arc::new(Mutex::new(None));
    let active_local_keys = Arc::new(Mutex::new(None));
    let pool = nmp_network::pool::Pool::new(
        nmp_network::pool::PoolConfig::default(),
        channel::<nmp_network::pool::PoolEvent>().0,
    );
    let mut relay_controls: HashMap<CanonicalRelayUrl, super::RelayControl> = HashMap::new();
    let mut slot_to_url: HashMap<u32, CanonicalRelayUrl> = HashMap::new();
    let mut connected_relays = HashSet::new();
    let mut connected_urls = HashSet::new();
    let mut last_emit = Instant::now();
    let mut next_relay_generation = 1u64;
    let mut running = true;
    let mut emit_hz = 4u32;
    let mut startup_sent = false;
    let mut parked_ops: Vec<ParkedOp> = Vec::new();
    let capability_callback: crate::capability_socket::CapabilityCallbackSlot =
        Arc::new(Mutex::new(None));
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
    let blocked_relays_slot = Arc::new(Mutex::new(crate::substrate::empty_blocked_relay_lookup()));
    let bootstrap_self_kinds_slot = Arc::new(Mutex::new(None));
    let routing_trace_slot = Arc::new(Mutex::new(None));
    let event_store_slot = Arc::new(Mutex::new(None));
    let routing_substrate_slot = Arc::new(Mutex::new(None));
    let publish_resolver_slot = Arc::new(Mutex::new(None));
    let active_account_slot = Arc::new(Mutex::new(None));
    let raw_event_forward_observer_ids =
        crate::actor::raw_event_forwarder::new_raw_event_forward_observer_id_slot();
    let raw_event_forward_policy_slot = Arc::new(Mutex::new(None));
    let raw_event_observers = commands::new_raw_event_observer_slot();

    let mut ctx = ActorContext {
        kernel,
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
        relays_ready: false,
        lifecycle_observer: &lifecycle_observer,
        mls_local_nsec: &mls_local_nsec,
        active_local_keys: &active_local_keys,
        capability_callback: &capability_callback,
        parked_ops: &mut parked_ops,
        command_tx_self: &command_tx,
        capability_work_tx: &capability_work_tx,
        coverage_hook_slot: &coverage_hook,
        req_frame_interceptor_slot: &req_frame_interceptor,
        host_op_handler: &host_op_handler,
        ingest_dispatcher_slot: &ingest_dispatcher_slot,
        dm_inbox_relays_slot: &dm_inbox_relays_slot,
        blocked_relays_slot: &blocked_relays_slot,
        bootstrap_self_kinds_slot: &bootstrap_self_kinds_slot,
        routing_trace_slot: &routing_trace_slot,
        event_store_slot: &event_store_slot,
        routing_substrate_slot: &routing_substrate_slot,
        publish_resolver_slot: &publish_resolver_slot,
        active_account_slot: &active_account_slot,
        raw_event_forward_observer_ids: &raw_event_forward_observer_ids,
        raw_event_forward_policy_slot: &raw_event_forward_policy_slot,
        raw_event_observers_handle: &raw_event_observers,
    };
    dispatch_command(cmd, &mut ctx);
    parked_ops
}
