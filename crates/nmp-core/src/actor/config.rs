use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};

use crate::capability_socket::CapabilityCallbackSlot;
use crate::kernel::Kernel;
use crate::slots::{
    ActiveAccountSlot, ActiveLocalKeysSlot, EventStoreSlot, KernelClockSlot, MlsLocalNsecSlot,
    ExternalEventSinkPolicyFactory, ExternalEventSinkPolicySlot,
    PublishResolverFactory, PublishResolverSlot, RoutingSubstrateFactory, RoutingSubstrateSlot,
    RoutingTraceSlot, StoragePathSlot,
};
use crate::subs::PlanCoverageHook;
use crate::substrate::{
    BlockedRelayLookup, ContactsLookup, DmInboxRelayLookup, EventIngestDispatcher, HostOpHandler,
    HostOpHandlerSlot, ProfileLookup, RelayConnectedHook, RelayConnectedHookSlot,
    RelayTextInterceptor, RelayTextInterceptorSlot, ReqFrameInterceptor, ReqFrameInterceptorSlot,
};
use crate::update_envelope::UpdateFrameBytes;

use super::{
    ActorMail, BunkerHandshakeSlot, CommandSender, KernelEventObserverSlot, LifecycleObserverSlot,
    SignerStateSlot,
};

pub struct ActorChannels {
    pub inbox_rx: Receiver<ActorMail>,
    pub command_tx_self: CommandSender,
    pub update_tx: Sender<UpdateFrameBytes>,
}

pub struct ActorRuntimeSlots {
    pub lifecycle_observer: LifecycleObserverSlot,
    pub event_observers: KernelEventObserverSlot,
    pub snapshot_projections: crate::kernel::SnapshotProjectionSlot,
    pub bunker_handshake: BunkerHandshakeSlot,
    pub signer_state: SignerStateSlot,
    pub bunker_hook: crate::bunker_hook::BunkerHookSlot,
    pub external_signer_hook: crate::external_signer_hook::ExternalSignerHookSlot,
    pub configured_relays: crate::kernel::AppRelaySlot,
    pub mls_local_nsec: MlsLocalNsecSlot,
    pub active_local_keys: ActiveLocalKeysSlot,
    pub capability_callback: CapabilityCallbackSlot,
    pub queue_depth: Arc<AtomicU64>,
    pub routing_trace: RoutingTraceSlot,
    pub active_account: ActiveAccountSlot,
    pub event_store: EventStoreSlot,
    pub external_event_sink_dispatcher: crate::substrate::ExternalEventSinkDispatcherSlot,
}

pub struct ActorConfigSources {
    pub storage_path: StoragePathSlot,
    pub coverage_hook: Arc<Mutex<Option<PlanCoverageHook>>>,
    pub req_frame_interceptor: ReqFrameInterceptorSlot,
    pub host_op_handler: HostOpHandlerSlot,
    pub relay_text_interceptor: RelayTextInterceptorSlot,
    pub relay_connected_hook: RelayConnectedHookSlot,
    pub ingest_dispatcher: Arc<RwLock<EventIngestDispatcher>>,
    pub dm_inbox_relays: Arc<Mutex<Arc<dyn DmInboxRelayLookup>>>,
    pub profile_lookup: Arc<Mutex<Arc<dyn ProfileLookup>>>,
    pub contacts_lookup: Arc<Mutex<Arc<dyn ContactsLookup>>>,
    pub blocked_relays: Arc<Mutex<Arc<dyn BlockedRelayLookup>>>,
    pub bootstrap_self_kinds: Arc<Mutex<Option<Vec<u64>>>>,
    pub routing_substrate: RoutingSubstrateSlot,
    pub publish_resolver: PublishResolverSlot,
    pub external_event_sink_policy: ExternalEventSinkPolicySlot,
    pub kernel_clock: KernelClockSlot,
}

impl ActorConfigSources {
    #[must_use]
    pub fn snapshot(self) -> ActorConfig {
        ActorConfig {
            storage_path: self
                .storage_path
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
            coverage_hook: self
                .coverage_hook
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
            req_frame_interceptor: self
                .req_frame_interceptor
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
            host_op_handler: self
                .host_op_handler
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().cloned()),
            relay_text_interceptors: self
                .relay_text_interceptor
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_default(),
            relay_connected_hooks: self
                .relay_connected_hook
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_default(),
            ingest_dispatcher: self.ingest_dispatcher,
            dm_inbox_relays: self
                .dm_inbox_relays
                .lock()
                .map(|guard| Arc::clone(&*guard))
                .unwrap_or_else(|_| crate::substrate::empty_dm_inbox_relay_lookup()),
            profile_lookup: self
                .profile_lookup
                .lock()
                .map(|guard| Arc::clone(&*guard))
                .unwrap_or_else(|_| crate::substrate::empty_profile_lookup()),
            contacts_lookup: self
                .contacts_lookup
                .lock()
                .map(|guard| Arc::clone(&*guard))
                .unwrap_or_else(|_| crate::substrate::empty_contacts_lookup()),
            blocked_relays: self
                .blocked_relays
                .lock()
                .map(|guard| Arc::clone(&*guard))
                .unwrap_or_else(|_| crate::substrate::empty_blocked_relay_lookup()),
            bootstrap_self_kinds: self.bootstrap_self_kinds.lock().ok().and_then(|guard| {
                guard
                    .as_ref()
                    .map(|kinds| kinds.iter().map(|kind| *kind as u32).collect())
            }),
            routing_substrate: self
                .routing_substrate
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(Arc::clone)),
            publish_resolver: self
                .publish_resolver
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(Arc::clone)),
            external_event_sink_policy: self
                .external_event_sink_policy
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(Arc::clone)),
            kernel_clock: self
                .kernel_clock
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(Arc::clone)),
        }
    }
}

pub struct ActorConfig {
    pub storage_path: Option<String>,
    pub coverage_hook: Option<PlanCoverageHook>,
    pub req_frame_interceptor: Option<Arc<dyn ReqFrameInterceptor>>,
    pub host_op_handler: Option<Arc<dyn HostOpHandler>>,
    pub relay_text_interceptors: Vec<Arc<dyn RelayTextInterceptor>>,
    pub relay_connected_hooks: Vec<Arc<dyn RelayConnectedHook>>,
    pub ingest_dispatcher: Arc<RwLock<EventIngestDispatcher>>,
    pub dm_inbox_relays: Arc<dyn DmInboxRelayLookup>,
    pub profile_lookup: Arc<dyn ProfileLookup>,
    pub contacts_lookup: Arc<dyn ContactsLookup>,
    pub blocked_relays: Arc<dyn BlockedRelayLookup>,
    pub bootstrap_self_kinds: Option<Vec<u32>>,
    pub routing_substrate: Option<Arc<RoutingSubstrateFactory>>,
    pub publish_resolver: Option<Arc<PublishResolverFactory>>,
    pub external_event_sink_policy: Option<Arc<ExternalEventSinkPolicyFactory>>,
    pub kernel_clock: Option<Arc<dyn crate::kernel::Clock>>,
}

impl ActorConfig {
    #[must_use]
    pub fn kernel_with_account_slot(
        &self,
        visible_limit: usize,
        active_account: ActiveAccountSlot,
    ) -> Kernel {
        Kernel::with_storage_path_and_account_slot(
            visible_limit,
            self.storage_path.as_deref(),
            active_account,
        )
    }

    pub fn apply_to_kernel(&self, kernel: &mut Kernel) {
        if let Some(factory) = &self.routing_substrate {
            let observer: Arc<dyn crate::substrate::RoutingTraceObserver> =
                kernel.routing_trace() as Arc<dyn crate::substrate::RoutingTraceObserver>;
            let (router, cache) = factory(observer);
            kernel.set_routing(router, cache);
        }
        if let Some(factory) = &self.publish_resolver {
            let resolver = factory(
                kernel.event_store_handle(),
                kernel.indexer_relays_handle(),
                kernel.local_write_relays_handle(),
                kernel.active_account_handle(),
            );
            kernel.set_publish_resolver(resolver);
        }
        if let Some(clock) = &self.kernel_clock {
            kernel.set_clock(Arc::clone(clock));
        }
        kernel.set_ingest_dispatcher_slot(Arc::clone(&self.ingest_dispatcher));
        kernel.set_dm_inbox_relay_lookup(Arc::clone(&self.dm_inbox_relays));
        kernel.set_profile_lookup(Arc::clone(&self.profile_lookup));
        kernel.set_contacts_lookup(Arc::clone(&self.contacts_lookup));
        kernel.set_blocked_relay_lookup(Arc::clone(&self.blocked_relays));
        kernel.set_bootstrap_self_kinds_override(self.bootstrap_self_kinds.clone());
        if let Some(hook) = &self.coverage_hook {
            kernel.lifecycle_mut().set_coverage_hook(Arc::clone(hook));
        }
        if let Some(interceptor) = &self.req_frame_interceptor {
            kernel
                .lifecycle_mut()
                .set_req_frame_interceptor(Arc::clone(interceptor));
        }
    }
}
