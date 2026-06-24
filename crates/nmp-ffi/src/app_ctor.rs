//! `nmp_app_new()` constructor — extracted from `lib.rs` to keep each file
//! under the 500-LOC ceiling (AGENTS.md file-size rule). No logic changes;
//! code moved verbatim.

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::passive_start::ActorStarter;
use nmp_core::__ffi_internal::{
    default_registry, new_app_relay_slot, new_bunker_handshake_slot, new_capability_callback_slot,
    new_event_observer_slot, new_lifecycle_observer_slot, new_signer_state_slot,
    new_snapshot_projection_slot, run_actor_with_observers, ActorChannels, ActorConfigSources,
    ActorRuntimeSlots,
};
use nmp_core::slots::{
    new_active_account_slot, new_active_local_keys_slot, new_event_store_slot,
    new_external_event_sink_policy_slot, new_mls_local_nsec_slot,
    new_nostrconnect_bootstrap_relay_slot, new_nostrconnect_perms_slot, new_publish_resolver_slot,
    new_pull_cursor_registry_handle_slot, new_routing_substrate_slot, new_routing_trace_slot,
    new_singleton_event_observer_id_slot, new_storage_path_slot,
};
use nmp_core::subs::PlanCoverageHook;
use nmp_core::substrate::new_external_event_sink_dispatcher_slot;

use crate::app_struct::{
    new_identity_change_observer_slot, new_search_relay_source_slot, new_update_callback_slot,
    notify_identity_change_observers, NmpApp,
};
use crate::app_sub_structs::{CapabilityPorts, CompositionConfig, ReadHandles};

#[no_mangle]
pub extern "C" fn nmp_app_new() -> *mut NmpApp {
    // ADR-0050 §D3a — one waking inbox of `ActorMail`. `command_tx` is the host
    // `CommandSender` (stored on `NmpApp`); the actor receives on `command_rx`.
    let (inbox_tx, command_rx) = std::sync::mpsc::channel::<nmp_core::__ffi_internal::ActorMail>();
    let command_tx = nmp_core::CommandSender::new(inbox_tx);
    let (update_tx, update_rx) = std::sync::mpsc::channel();
    let update_callback = new_update_callback_slot();
    let listener_callback = Arc::clone(&update_callback);
    // T118 / G3 — shared lifecycle observer slot.
    let lifecycle_observer = new_lifecycle_observer_slot();
    let actor_lifecycle_observer = Arc::clone(&lifecycle_observer);
    // T146 — shared kernel event observer slot.
    let event_observers = new_event_observer_slot();
    let actor_event_observers = Arc::clone(&event_observers);
    // Per-app idempotency slot — tracks the previously-installed singleton
    // kernel-event observer id for a per-app crate that wants exactly one
    // auxiliary `KernelEventObserver` per app. NOT shared with the actor thread.
    let singleton_event_observer_id = new_singleton_event_observer_id_slot();
    // Host-extensible snapshot output slot.
    let snapshot_projections = new_snapshot_projection_slot();
    let actor_snapshot_projections = Arc::clone(&snapshot_projections);
    // V-38: relay-text interceptor slot (actor clone + `NmpApp` clone).
    let relay_text_interceptor = nmp_core::substrate::new_relay_text_interceptor_slot();
    let actor_relay_text_interceptor = Arc::clone(&relay_text_interceptor);
    // ADR-0051: relay-connected hook slot (actor clone + `NmpApp` clone).
    let relay_connected_hook = nmp_core::substrate::new_relay_connected_hook_slot();
    let actor_relay_connected_hook = Arc::clone(&relay_connected_hook);
    // ADR-0052 §D3: per-app signer hook slots.
    let bunker_hook = nmp_core::new_bunker_hook_slot();
    let actor_bunker_hook = Arc::clone(&bunker_hook);
    let external_signer_hook = nmp_core::new_external_signer_hook_slot();
    let actor_external_signer_hook = Arc::clone(&external_signer_hook);
    // D0: bunker-handshake slot — handed to the actor for built-in projection.
    let actor_bunker_handshake = new_bunker_handshake_slot();
    // ADR-0048 D6: unified remote-signer health slot.
    let actor_signer_state = new_signer_state_slot();
    // Shared relay-edit rows handle.
    let configured_relays: nmp_core::AppRelaySlot = new_app_relay_slot();
    let actor_configured_relays = Arc::clone(&configured_relays);
    // V-65 — NIP-46 bootstrap relay slot.
    let nostrconnect_bootstrap_relay = new_nostrconnect_bootstrap_relay_slot();
    // #1493 P9 — NIP-46 perm request slot.
    let nostrconnect_perms = new_nostrconnect_perms_slot();
    // Active local (nsec) key slot.
    let mls_local_nsec = new_mls_local_nsec_slot();
    let actor_mls_local_nsec = Arc::clone(&mls_local_nsec);
    // Active local `nostr::Keys` slot — substrate-generic.
    let active_local_keys = new_active_local_keys_slot();
    let actor_active_local_keys = Arc::clone(&active_local_keys);
    // V-82 — active-account hex-pubkey slot.
    let active_account_handle = new_active_account_slot();
    let actor_active_account = Arc::clone(&active_account_handle);
    let identity_change_observers = new_identity_change_observer_slot();
    let listener_identity_change_observers = Arc::clone(&identity_change_observers);
    let listener_active_account = Arc::clone(&active_account_handle);
    let listener_last_active_account = Arc::new(Mutex::new(None));
    // V-83 — event-store publish-back slot.
    let event_store_handle = new_event_store_slot();
    let actor_event_store = Arc::clone(&event_store_handle);
    // ADR-0058 step 3b — pull-cursor registry publish-back slot.
    let pull_cursor_registry = new_pull_cursor_registry_handle_slot();
    let actor_pull_cursor_registry = Arc::clone(&pull_cursor_registry);
    // Shared capability callback slot.
    let capability_callback = new_capability_callback_slot();
    let actor_capability_callback = Arc::clone(&capability_callback);
    // FFI-supplied LMDB storage path slot.
    let storage_path = new_storage_path_slot();
    let actor_storage_path = Arc::clone(&storage_path);
    // V-51 phase 4 — shared routing-trace projection slot.
    let routing_trace = new_routing_trace_slot();
    let actor_routing_trace = Arc::clone(&routing_trace);
    // V-51 phase 5 — substrate-routing factory slot.
    let routing_substrate = new_routing_substrate_slot();
    let actor_routing_substrate = Arc::clone(&routing_substrate);
    // ADR-0049 Part 2 — the composition ledger.
    let composition_ledger: Arc<nmp_core::CompositionLedger> =
        Arc::new(nmp_core::CompositionLedger::new());
    // Spec §271 (2026-05-25) — substrate-publish-resolver factory slot.
    let publish_resolver = new_publish_resolver_slot();
    let actor_publish_resolver = Arc::clone(&publish_resolver);
    // Test-support kernel-clock injection slot.
    let kernel_clock = nmp_core::slots::new_kernel_clock_slot();
    let actor_kernel_clock = Arc::clone(&kernel_clock);
    let external_event_sink_policy = new_external_event_sink_policy_slot();
    let actor_external_event_sink_policy = Arc::clone(&external_event_sink_policy);
    let external_event_sink_dispatcher_slot = new_external_event_sink_dispatcher_slot();
    // Publish a constructed (but not-yet-bound) dispatcher into the slot NOW.
    if let Ok(mut guard) = external_event_sink_dispatcher_slot.lock() {
        *guard = Some(nmp_core::substrate::ExternalEventSinkDispatcher::new());
    }
    let actor_external_event_sink_dispatcher_slot =
        Arc::clone(&external_event_sink_dispatcher_slot);
    let feed_registry = nmp_feed::new_feed_registry_slot();
    let feed_sessions = Arc::new(nmp_feed::FeedSessionRegistry::default());
    // One-shot MLS-autopublish intent flag.
    let pending_mls_autopublish = AtomicBool::new(false);
    // G-S4 — actor command-channel depth straddle counter.
    let queue_depth: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let actor_queue_depth = Arc::clone(&queue_depth);
    // D2 — shared coverage-gate hook slot.
    let coverage_hook: Arc<Mutex<Option<PlanCoverageHook>>> = Arc::new(Mutex::new(None));
    let actor_coverage_hook = Arc::clone(&coverage_hook);
    let req_frame_interceptor = nmp_core::substrate::new_req_frame_interceptor_slot();
    let actor_req_frame_interceptor = Arc::clone(&req_frame_interceptor);
    // V-40 — substrate `EventIngestDispatcher` slot.
    let ingest_dispatcher_slot: Arc<std::sync::RwLock<nmp_core::substrate::EventIngestDispatcher>> =
        Arc::new(std::sync::RwLock::new(
            nmp_core::substrate::EventIngestDispatcher::new(),
        ));
    let actor_ingest_dispatcher = Arc::clone(&ingest_dispatcher_slot);
    // #1811 — crate-registered FTS scope registry.
    let search_scope_registry: Arc<nmp_core::substrate::SearchScopeRegistry> =
        Arc::new(nmp_core::substrate::SearchScopeRegistry::new());
    let actor_search_scope_registry = Arc::clone(&search_scope_registry);
    // #1804 — crate-registered input-scope recognizer registry.
    let input_scope_registry: Arc<nmp_core::substrate::InputScopeRegistry> =
        Arc::new(nmp_core::substrate::InputScopeRegistry::new());
    // V-40 — substrate `DmInboxRelayLookup` slot.
    let dm_inbox_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::DmInboxRelayLookup>>> =
        Arc::new(Mutex::new(
            nmp_core::substrate::empty_dm_inbox_relay_lookup(),
        ));
    let actor_dm_inbox_relays = Arc::clone(&dm_inbox_relays_slot);
    // ADR-0057 PR 2 — substrate `ProfileLookup` slot.
    let profile_lookup_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::ProfileLookup>>> =
        Arc::new(Mutex::new(nmp_core::substrate::empty_profile_lookup()));
    let actor_profile_lookup = Arc::clone(&profile_lookup_slot);
    // ADR-0057 PR 3 — substrate `ContactsLookup` slot.
    let contacts_lookup_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::ContactsLookup>>> =
        Arc::new(Mutex::new(nmp_core::substrate::empty_contacts_lookup()));
    let actor_contacts_lookup = Arc::clone(&contacts_lookup_slot);
    // Blocked-relay lookup slot.
    let blocked_relays_slot: Arc<Mutex<Arc<dyn nmp_core::substrate::BlockedRelayLookup>>> =
        Arc::new(Mutex::new(nmp_core::substrate::empty_blocked_relay_lookup()));
    let actor_blocked_relays = Arc::clone(&blocked_relays_slot);
    // Per-app override for the bootstrap Tailing self-kinds list.
    let bootstrap_self_kinds: Arc<Mutex<Option<Vec<u64>>>> = Arc::new(Mutex::new(None));
    let actor_bootstrap_self_kinds = Arc::clone(&bootstrap_self_kinds);
    // Clone so we can report actor death through the same listener pipe.
    let update_tx_panic = update_tx.clone();
    let startup_update_tx = update_tx.clone();
    // Test-support GC budget ceiling slot.
    #[cfg(any(test, feature = "test-support"))]
    let gc_budget_ceiling: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
    #[cfg(any(test, feature = "test-support"))]
    let actor_gc_budget_ceiling = Arc::clone(&gc_budget_ceiling);

    let actor_command_tx_self = command_tx.clone();
    let actor_starter: ActorStarter = Box::new(move || {
        let channels = ActorChannels {
            inbox_rx: command_rx,
            command_tx_self: actor_command_tx_self,
            update_tx,
        };
        let runtime = ActorRuntimeSlots {
            lifecycle_observer: actor_lifecycle_observer,
            event_observers: actor_event_observers,
            snapshot_projections: actor_snapshot_projections,
            bunker_handshake: actor_bunker_handshake,
            signer_state: actor_signer_state,
            bunker_hook: actor_bunker_hook,
            external_signer_hook: actor_external_signer_hook,
            configured_relays: actor_configured_relays,
            mls_local_nsec: actor_mls_local_nsec,
            active_local_keys: actor_active_local_keys,
            capability_callback: actor_capability_callback,
            queue_depth: actor_queue_depth,
            routing_trace: actor_routing_trace,
            active_account: actor_active_account,
            event_store: actor_event_store,
            pull_cursor_registry: actor_pull_cursor_registry,
            external_event_sink_dispatcher: actor_external_event_sink_dispatcher_slot,
        };
        // Compute GC budget ceiling at start time (after nmp_app_configure_gc_budget).
        #[cfg(any(test, feature = "test-support"))]
        let gc_budget_ceiling_for_config: Option<usize> =
            actor_gc_budget_ceiling.lock().ok().and_then(|g| *g);
        #[cfg(not(any(test, feature = "test-support")))]
        let gc_budget_ceiling_for_config: Option<usize> = None;

        let config = ActorConfigSources {
            storage_path: actor_storage_path,
            coverage_hook: actor_coverage_hook,
            req_frame_interceptor: actor_req_frame_interceptor,
            relay_text_interceptor: actor_relay_text_interceptor,
            relay_connected_hook: actor_relay_connected_hook,
            ingest_dispatcher: actor_ingest_dispatcher,
            search_scope_registry: actor_search_scope_registry,
            dm_inbox_relays: actor_dm_inbox_relays,
            profile_lookup: actor_profile_lookup,
            contacts_lookup: actor_contacts_lookup,
            blocked_relays: actor_blocked_relays,
            bootstrap_self_kinds: actor_bootstrap_self_kinds,
            routing_substrate: actor_routing_substrate,
            publish_resolver: actor_publish_resolver,
            external_event_sink_policy: actor_external_event_sink_policy,
            kernel_clock: actor_kernel_clock,
            gc_budget_ceiling: gc_budget_ceiling_for_config,
        }
        .snapshot();
        thread::spawn(move || {
            // D7 (actor-death visibility): catch unwind here and emit one
            // envelope-conforming `Panic` frame on the update channel before this
            // thread (and `update_tx`) is dropped.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_actor_with_observers(channels, config, runtime);
            }));
            if let Err(e) = result {
                let msg = nmp_core::panic_message(&*e);
                let frame = nmp_core::encode_panic(format!("actor thread died: {msg}"));
                let _ = update_tx_panic.send(frame);
            }
        })
    });
    let (embed_sidecar, listener_embed_sidecar) =
        crate::snapshot::embed_sidecar::new_embed_sidecar_pair();
    let update_listener = thread::spawn(move || {
        while let Ok(update) = update_rx.recv() {
            crate::snapshot::embed_sidecar::update_embed_sidecar_from_frame(
                &update,
                &listener_embed_sidecar,
            );
            notify_identity_change_observers(
                &listener_active_account,
                &listener_last_active_account,
                &listener_identity_change_observers,
            );
            // Quiescence-safe callback invocation (option b — Condvar drain).
            let registration = {
                // D6 fail-loud: recover from poisoned lock rather than silently
                // skipping the update delivery.
                let mut guard = listener_callback.inner.lock().unwrap_or_else(|e| {
                    tracing::error!("listener lock was poisoned; recovering");
                    e.into_inner()
                });
                let reg = guard.registration;
                if reg.is_some() {
                    guard.in_flight += 1;
                }
                reg
            };
            if let Some(registration) = registration {
                // UB guard: the foreign update callback may panic / raise.
                let _ = nmp_core::ffi_guard::guard_ffi_callback("update listener", || {
                    (registration.callback)(
                        registration.context as *mut std::ffi::c_void,
                        update.as_ptr(),
                        update.len(),
                    );
                });
                // Decrement in_flight and wake any waiting setter.
                let mut guard = listener_callback.inner.lock().unwrap_or_else(|e| {
                    tracing::error!("listener lock was poisoned; recovering");
                    e.into_inner()
                });
                guard.in_flight = guard.in_flight.saturating_sub(1);
                if guard.in_flight == 0 {
                    listener_callback.drained.notify_all();
                }
            }
        }
    });
    let app = NmpApp {
        tx: command_tx,
        update_callback,
        identity_change_observers,
        capability_callback,
        lifecycle_observer,
        event_observers,
        singleton_event_observer_id,
        configured_relays,
        pending_mls_autopublish,
        actor_starter: Mutex::new(Some(actor_starter)),
        startup_update_tx: Mutex::new(Some(startup_update_tx)),
        actor: Mutex::new(None),
        update_listener: Mutex::new(Some(update_listener)),
        // M6 — action registry seeded with PublishModule; composition ledger attached.
        action_registry: default_registry()
            .with_composition_ledger(Arc::clone(&composition_ledger)),
        composition_ledger,
        // ADR-0049 Part 2 — not started until `nmp_app_start` sends Start.
        started: AtomicBool::new(false),
        snapshot_projections,
        feed_registry,
        // #1740 step 2 — empty until the first `open_feed`.
        feed_sessions,
        // #1740 step 4 — empty until the first `register_custom_perspective`.
        custom_perspectives: Arc::new(nmp_feed::PerspectiveRegistry::default()),
        interest_feed_observers: Mutex::new(std::collections::HashMap::new()),
        queue_depth,
        #[cfg(test)]
        send_cmd_count: AtomicU64::new(0),
        #[cfg(test)]
        last_cmd_tag: std::sync::Mutex::new(None),
        #[cfg(feature = "signer-broker")]
        signer_broker: Arc::new(Mutex::new(None)),
        #[cfg(feature = "external-signer")]
        external_signer_driver: Arc::new(Mutex::new(None)),
        search_sessions: Mutex::new(std::collections::HashMap::new()),
        #[cfg(any(test, feature = "test-support"))]
        gc_budget_ceiling,
        composition: CompositionConfig {
            storage_path,
            nostrconnect_bootstrap_relay,
            nostrconnect_perms,
            initial_relays_for_start: Mutex::new(Vec::new()),
            coverage_hook,
            req_frame_interceptor,
            relay_text_interceptor,
            relay_connected_hook,
            kernel_clock,
            external_event_sink_policy,
            routing_substrate,
            publish_resolver,
            bootstrap_self_kinds,
            dm_inbox_relays_slot,
            profile_lookup_slot,
            contacts_lookup_slot,
            blocked_relays_slot,
            mailbox_cache_reader: Mutex::new(None),
            search_scope_registry,
            input_scope_registry,
            bunker_hook,
        },
        capability_ports: CapabilityPorts {
            ingest_dispatcher_slot,
            search_relay_source: new_search_relay_source_slot(),
            external_signer_hook,
        },
        read_handles: ReadHandles {
            event_store_handle,
            pull_cursor_registry,
            routing_trace,
            active_account_handle,
            active_local_keys,
            mls_local_nsec,
        },
    };
    // D0 — the built-in `"bunker_handshake"` projection is registered inside
    // `run_actor_with_observers` (at the actor wiring site), not here.
    crate::snapshot::embed_sidecar::install_embed_sidecar_projection(&app, embed_sidecar);

    // Issue #1238: install the per-app NIP-55 restore hook before any host can
    // send `Start`.
    #[cfg(feature = "external-signer")]
    crate::external_signer::init_external_signer_driver(&app);
    Box::into_raw(Box::new(app))
}
