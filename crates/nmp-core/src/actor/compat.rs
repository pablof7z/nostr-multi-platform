//! Backwards-compatible actor entry points.
//!
//! The full FFI/app path calls `run_actor_with_observers` with slots owned by
//! `NmpApp`. These shims are for older tests and the `nmp-core::testing` facade,
//! so they provide private throwaway slots. In test-support builds, the
//! profile/contacts slots must still be real parser/cache pairs; otherwise the
//! shim would override `Kernel::new`'s read-your-writes defaults with empty
//! lookup objects.

use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, Sender};

use super::{
    new_bunker_handshake_slot, new_capability_callback_slot, new_event_observer_slot,
    new_lifecycle_observer_slot, new_raw_event_observer_slot, new_signer_state_slot, ActorMail,
    CommandSender, LifecycleObserverSlot,
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
    super::run_actor_with_observers(
        inbox_rx,
        command_tx_self,
        update_tx,
        lifecycle_observer,
        new_event_observer_slot(),
        new_raw_event_observer_slot(),
        crate::kernel::new_snapshot_projection_slot(),
        crate::substrate::new_relay_text_interceptor_slot(),
        crate::substrate::new_relay_connected_hook_slot(),
        new_bunker_handshake_slot(),
        new_signer_state_slot(),
        crate::bunker_hook::new_bunker_hook_slot(),
        crate::external_signer_hook::new_external_signer_hook_slot(),
        crate::kernel::new_app_relay_slot(),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        new_capability_callback_slot(),
        Arc::new(Mutex::new(None)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(Mutex::new(None)),
        crate::substrate::new_req_frame_interceptor_slot(),
        crate::substrate::new_host_op_handler_slot(),
        ingest_dispatcher,
        Arc::new(Mutex::new(crate::substrate::empty_dm_inbox_relay_lookup())),
        profile_lookup,
        contacts_lookup,
        Arc::new(Mutex::new(crate::substrate::empty_blocked_relay_lookup())),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        Arc::new(Mutex::new(None)),
        crate::slots::new_raw_event_forward_policy_slot(),
        crate::slots::new_active_account_slot(),
        crate::slots::new_event_store_slot(),
        crate::slots::new_kernel_clock_slot(),
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
        Arc::new(crate::substrate::TestKind0Parser::new(Arc::clone(&profile_cache))),
    );
    dispatcher.register_kind(
        3,
        Arc::new(crate::substrate::TestKind3Parser::new(Arc::clone(&contacts_cache))),
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
