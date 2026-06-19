//! Backwards-compatible actor entry points.
//!
//! The full FFI/app path calls `run_actor_with_observers` with slots owned by
//! `NmpApp`. These shims are for older tests and the `nmp-core::testing` facade,
//! so they provide private throwaway slots. In test-support builds, the
//! profile/contacts slots must still be real parser/cache pairs; otherwise the
//! shim would override `Kernel::new`'s read-your-writes defaults with empty
//! lookup objects.

use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};

use super::{
    new_bunker_handshake_slot, new_capability_callback_slot, new_event_observer_slot,
    new_lifecycle_observer_slot, new_signer_state_slot, ActorChannels,
    ActorConfigSources, ActorMail, ActorRuntimeSlots, CommandSender, LifecycleObserverSlot,
};

/// Backwards-compatible entry point: spawn the actor without a lifecycle
/// observer. Existing tests and the `nmp-core::testing` facade call this shape.
#[allow(dead_code)]
pub fn run_actor(
    inbox_rx: Receiver<ActorMail>,
    command_tx_self: CommandSender,
    update_tx: Sender<crate::update_envelope::UpdateFrameBytes>,
) {
    run_actor_with_lifecycle_observer(
        inbox_rx,
        command_tx_self,
        update_tx,
        new_lifecycle_observer_slot(),
    );
}

/// T118 / G3 backwards-compatible entry point. Spawns the actor with a lifecycle
/// observer but no kernel event observer slot.
#[allow(dead_code)]
pub fn run_actor_with_lifecycle_observer(
    inbox_rx: Receiver<ActorMail>,
    command_tx_self: CommandSender,
    update_tx: Sender<crate::update_envelope::UpdateFrameBytes>,
    lifecycle_observer: LifecycleObserverSlot,
) {
    let (ingest_dispatcher, profile_lookup, contacts_lookup) = private_ingest_slots();
    let runtime = ActorRuntimeSlots {
        lifecycle_observer,
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
        ingest_dispatcher,
        dm_inbox_relays: Arc::new(Mutex::new(crate::substrate::empty_dm_inbox_relay_lookup())),
        profile_lookup,
        contacts_lookup,
        blocked_relays: Arc::new(Mutex::new(crate::substrate::empty_blocked_relay_lookup())),
        bootstrap_self_kinds: Arc::new(Mutex::new(None)),
        routing_substrate: crate::slots::new_routing_substrate_slot(),
        publish_resolver: crate::slots::new_publish_resolver_slot(),
        external_event_sink_policy: crate::slots::new_external_event_sink_policy_slot(),
        kernel_clock: crate::slots::new_kernel_clock_slot(),
        // No GC budget ceiling for the compat shim — production default (disabled).
        gc_budget_ceiling: None,
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

type IngestSlots = (
    Arc<RwLock<crate::substrate::EventIngestDispatcher>>,
    Arc<Mutex<Arc<dyn crate::substrate::ProfileLookup>>>,
    Arc<Mutex<Arc<dyn crate::substrate::ContactsLookup>>>,
);

#[cfg(any(test, feature = "test-support"))]
fn private_ingest_slots() -> IngestSlots {
    let profile_cache = Arc::new(crate::substrate::TestProfileCache::new());
    let contacts_cache = Arc::new(crate::substrate::TestContactsCache::new());
    let mut dispatcher = crate::substrate::EventIngestDispatcher::new();
    dispatcher.register_kind(
        0,
        Arc::new(crate::substrate::TestKind0Parser::new(Arc::clone(
            &profile_cache,
        ))),
    );
    dispatcher.register_kind(
        3,
        Arc::new(crate::substrate::TestKind3Parser::new(Arc::clone(
            &contacts_cache,
        ))),
    );
    let profile_lookup: Arc<dyn crate::substrate::ProfileLookup> = profile_cache;
    let contacts_lookup: Arc<dyn crate::substrate::ContactsLookup> = contacts_cache;
    (
        Arc::new(RwLock::new(dispatcher)),
        Arc::new(Mutex::new(profile_lookup)),
        Arc::new(Mutex::new(contacts_lookup)),
    )
}

#[cfg(not(any(test, feature = "test-support")))]
fn private_ingest_slots() -> IngestSlots {
    (
        Arc::new(RwLock::new(crate::substrate::EventIngestDispatcher::new())),
        Arc::new(Mutex::new(crate::substrate::empty_profile_lookup())),
        Arc::new(Mutex::new(crate::substrate::empty_contacts_lookup())),
    )
}
