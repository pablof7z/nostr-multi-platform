use std::sync::atomic::AtomicU64;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread;

use nmp_core::__ffi_internal::{
    new_bunker_handshake_slot, new_event_observer_slot, new_lifecycle_observer_slot,
    new_signer_state_slot, run_actor_with_observers, ActorChannels, ActorConfigSources,
    ActorRuntimeSlots,
};
use nmp_core::substrate::{EventIngestDispatcher, IngestParser, ProfileLookup};

pub fn spawn_actor_with_nip01_profile_cache() -> (
    nmp_core::CommandSender,
    mpsc::Receiver<nmp_core::UpdateFrameBytes>,
) {
    let profile_cache = Arc::new(nmp_nip01::ProfileCache::new());
    let mut dispatcher = EventIngestDispatcher::new();
    let parser: Arc<dyn IngestParser> =
        Arc::new(nmp_nip01::Kind0Parser::new(Arc::clone(&profile_cache)));
    dispatcher.register_kind(0, parser);
    let profile_lookup: Arc<dyn ProfileLookup> = profile_cache;

    let (command_tx, command_rx) = nmp_core::CommandSender::bounded_channel();
    let (update_tx, update_rx) = mpsc::channel();
    let actor_command_tx_self = command_tx.clone();

    thread::spawn(move || {
        let runtime = ActorRuntimeSlots {
            lifecycle_observer: new_lifecycle_observer_slot(),
            event_observers: new_event_observer_slot(),
            snapshot_projections: nmp_core::__ffi_internal::new_snapshot_projection_slot(),
            bunker_handshake: new_bunker_handshake_slot(),
            signer_state: new_signer_state_slot(),
            bunker_hook: nmp_core::new_bunker_hook_slot(),
            external_signer_hook: nmp_core::new_external_signer_hook_slot(),
            configured_relays: nmp_core::__ffi_internal::new_app_relay_slot(),
            mls_local_nsec: nmp_core::slots::new_mls_local_nsec_slot(),
            active_local_keys: nmp_core::slots::new_active_local_keys_slot(),
            capability_callback: nmp_core::__ffi_internal::new_capability_callback_slot(),
            queue_depth: Arc::new(AtomicU64::new(0)),
            routing_trace: nmp_core::slots::new_routing_trace_slot(),
            active_account: nmp_core::slots::new_active_account_slot(),
            event_store: nmp_core::slots::new_event_store_slot(),
            pull_cursor_registry: nmp_core::slots::new_pull_cursor_registry_handle_slot(),
            external_event_sink_dispatcher:
                nmp_core::substrate::new_external_event_sink_dispatcher_slot(),
        };
        let config = ActorConfigSources {
            storage_path: nmp_core::slots::new_storage_path_slot(),
            coverage_hook: Arc::new(Mutex::new(None)),
            req_frame_interceptor: nmp_core::substrate::new_req_frame_interceptor_slot(),
            host_op_handler: nmp_core::substrate::new_host_op_handler_slot(),
            relay_text_interceptor: nmp_core::substrate::new_relay_text_interceptor_slot(),
            relay_connected_hook: nmp_core::substrate::new_relay_connected_hook_slot(),
            ingest_dispatcher: Arc::new(RwLock::new(dispatcher)),
            search_scope_registry: Arc::new(nmp_core::substrate::SearchScopeRegistry::new()),
            dm_inbox_relays: Arc::new(Mutex::new(
                nmp_core::substrate::empty_dm_inbox_relay_lookup(),
            )),
            contact_list_reader: nmp_core::slots::new_contact_list_reader_slot(),
            profile_lookup: Arc::new(Mutex::new(profile_lookup)),
            blocked_relays: Arc::new(Mutex::new(nmp_core::substrate::empty_blocked_relay_lookup())),
            bootstrap_self_kinds: Arc::new(Mutex::new(None)),
            routing_substrate: nmp_core::slots::new_routing_substrate_slot(),
            publish_resolver: nmp_core::slots::new_publish_resolver_slot(),
            external_event_sink_policy: nmp_core::slots::new_external_event_sink_policy_slot(),
            kernel_clock: nmp_core::slots::new_kernel_clock_slot(),
            gc_budget_ceiling: None,
            user_agent: Arc::new(Mutex::new(None)),
            outbound_public_tags: Arc::new(Mutex::new(None)),
        }
        .snapshot();
        run_actor_with_observers(
            ActorChannels {
                inbox_rx: command_rx,
                command_tx_self: actor_command_tx_self,
                update_tx,
            },
            config,
            runtime,
        );
    });

    (command_tx, update_rx)
}
