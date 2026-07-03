//! Test-only actor spawn helper.
//!
//! This helper reproduces the backwards-compatible actor entry points
//! (`run_actor` and `run_actor_with_lifecycle_observer`) for tests that need
//! test-support lookup collaborators wired in. Production code must call
//! `run_actor_with_observers` directly.

use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};

use super::{
    new_bunker_handshake_slot, new_event_observer_slot, new_lifecycle_observer_slot,
    new_signer_state_slot, ActorChannels, ActorConfigSources, ActorMail, ActorRuntimeSlots,
    CommandSender,
};
use crate::capability_socket::new_capability_callback_slot;

/// Spawn the actor with test-support lookup seams wired in.
///
/// This helper provides the backwards-compatible entry point shape (`run_actor`)
/// for tests that need the core `ProfileLookup` seam.
/// Calling code must construct the actor through this function to ensure
/// `Kernel::new`'s test lookup defaults are preserved.
pub fn spawn_test_actor(
    inbox_rx: Receiver<ActorMail>,
    command_tx_self: CommandSender,
    update_tx: Sender<crate::update_envelope::UpdateFrameBytes>,
) {
    let profile_lookup = Arc::new(crate::substrate::TestProfileLookup::new());
    let dispatcher = crate::substrate::EventIngestDispatcher::new();
    let profile_lookup: Arc<dyn crate::substrate::ProfileLookup> = profile_lookup;

    let runtime = ActorRuntimeSlots {
        lifecycle_observer: new_lifecycle_observer_slot(),
        event_observers: new_event_observer_slot(),
        snapshot_projections: crate::kernel::new_snapshot_projection_slot(),
        bunker_handshake: new_bunker_handshake_slot(),
        signer_state: new_signer_state_slot(),
        bunker_hook: crate::bunker_hook::new_bunker_hook_slot(),
        external_signer_hook: crate::external_signer_hook::new_external_signer_hook_slot(),
        configured_relays: crate::kernel::new_app_relay_slot(),
        mls_local_nsec: Arc::new(Mutex::new(None)),
        active_local_keys: Arc::new(Mutex::new(None)),
        capability_callback: new_capability_callback_slot(),
        queue_depth: Arc::new(AtomicU64::new(0)),
        routing_trace: Arc::new(Mutex::new(None)),
        active_account: crate::slots::new_active_account_slot(),
        event_store: crate::slots::new_event_store_slot(),
        pull_cursor_registry: crate::slots::new_pull_cursor_registry_handle_slot(),
        external_event_sink_dispatcher: crate::substrate::new_external_event_sink_dispatcher_slot(),
    };
    let config = ActorConfigSources {
        storage_path: Arc::new(Mutex::new(None)),
        coverage_hook: Arc::new(Mutex::new(None)),
        req_frame_interceptor: crate::substrate::new_req_frame_interceptor_slot(),
        host_op_handler: crate::substrate::new_host_op_handler_slot(),
        relay_text_interceptor: crate::substrate::new_relay_text_interceptor_slot(),
        relay_connected_hook: crate::substrate::new_relay_connected_hook_slot(),
        ingest_dispatcher: Arc::new(RwLock::new(dispatcher)),
        search_scope_registry: Arc::new(crate::substrate::SearchScopeRegistry::new()),
        dm_inbox_relays: Arc::new(Mutex::new(crate::substrate::empty_dm_inbox_relay_lookup())),
        contact_list_reader: crate::slots::new_contact_list_reader_slot(),
        profile_lookup: Arc::new(Mutex::new(profile_lookup)),
        blocked_relays: Arc::new(Mutex::new(crate::substrate::empty_blocked_relay_lookup())),
        bootstrap_self_kinds: Arc::new(Mutex::new(None)),
        routing_substrate: crate::slots::new_routing_substrate_slot(),
        publish_resolver: crate::slots::new_publish_resolver_slot(),
        external_event_sink_policy: crate::slots::new_external_event_sink_policy_slot(),
        kernel_clock: crate::slots::new_kernel_clock_slot(),
        // No GC budget ceiling for test helper — production default (disabled).
        gc_budget_ceiling: None,
        user_agent: Arc::new(Mutex::new(None)),
        outbound_public_tags: Arc::new(Mutex::new(None)),
    }
    .snapshot();

    super::run_actor_with_observers(
        ActorChannels {
            inbox_rx,
            command_tx_self,
            update_tx,
        },
        config,
        runtime,
    );
}
