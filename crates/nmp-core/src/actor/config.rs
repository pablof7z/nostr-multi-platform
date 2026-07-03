use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};

use crate::capability_socket::CapabilityCallbackSlot;
use crate::kernel::Kernel;
use crate::slots::{
    ActiveAccountSlot, ActiveLocalKeysSlot, ContactListReader, ContactListReaderSlot,
    EventStoreSlot, ExternalEventSinkPolicyFactory, ExternalEventSinkPolicySlot, KernelClockSlot,
    MlsLocalNsecSlot, PublishResolverFactory, PublishResolverSlot, PullCursorRegistryHandleSlot,
    RoutingSubstrateFactory, RoutingSubstrateSlot, RoutingTraceSlot, StoragePathSlot,
};
use crate::subs::PlanCoverageHook;
use crate::substrate::{
    BlockedRelayLookup, DmInboxRelayLookup, EventIngestDispatcher, HostOpHandler,
    HostOpHandlerSlot, ProfileLookup, RelayConnectedHook, RelayConnectedHookSlot,
    RelayTextInterceptor, RelayTextInterceptorSlot, ReqFrameInterceptor, ReqFrameInterceptorSlot,
    SearchScopeRegistry,
};
use crate::update_envelope::UpdateFrameBytes;

use super::{
    ActorMail, BunkerHandshakeSlot, CommandSender, LifecycleObserverSlot,
    ObservedProjectionSinkSlot, SignerStateSlot,
};

pub struct ActorChannels {
    pub inbox_rx: Receiver<ActorMail>,
    pub command_tx_self: CommandSender,
    pub update_tx: Sender<UpdateFrameBytes>,
}

pub struct ActorRuntimeSlots {
    pub lifecycle_observer: LifecycleObserverSlot,
    pub event_observers: ObservedProjectionSinkSlot,
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
    /// ADR-0072 step 3b — publish-back of the kernel's pull-cursor registry.
    pub pull_cursor_registry: PullCursorRegistryHandleSlot,
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
    /// #1811 — crate-registered FTS scope registry. Compiled + installed into
    /// the kernel store at `apply_to_kernel` (after the store exists). Mirrors
    /// `ingest_dispatcher`: a shared `Arc` written by host registration,
    /// consumed once at composition.
    pub search_scope_registry: Arc<SearchScopeRegistry>,
    pub dm_inbox_relays: Arc<Mutex<Arc<dyn DmInboxRelayLookup>>>,
    pub contact_list_reader: ContactListReaderSlot,
    pub profile_lookup: Arc<Mutex<Arc<dyn ProfileLookup>>>,
    pub blocked_relays: Arc<Mutex<Arc<dyn BlockedRelayLookup>>>,
    pub bootstrap_self_kinds: Arc<Mutex<Option<Vec<u64>>>>,
    pub routing_substrate: RoutingSubstrateSlot,
    pub publish_resolver: PublishResolverSlot,
    pub external_event_sink_policy: ExternalEventSinkPolicySlot,
    pub kernel_clock: KernelClockSlot,
    /// Test-support durable-LRU ceiling for GC.  Set by
    /// `nmp_app_configure_gc_budget` before start; `None` (the default)
    /// preserves `GcBudget::production()` (`max_total_events = usize::MAX`,
    /// LRU disabled). Always `None` in production builds — applied to the
    /// kernel only through the `#[cfg(test-support)]`-gated
    /// `Kernel::set_gc_budget_ceiling` method in `apply_to_kernel`.
    pub gc_budget_ceiling: Option<usize>,
    pub user_agent: Arc<Mutex<Option<String>>>,
    pub outbound_public_tags: Arc<Mutex<Option<Vec<Vec<String>>>>>,
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
            search_scope_registry: self.search_scope_registry,
            dm_inbox_relays: self
                .dm_inbox_relays
                .lock()
                .map(|guard| Arc::clone(&*guard))
                .unwrap_or_else(|_| crate::substrate::empty_dm_inbox_relay_lookup()),
            contact_list_reader: self
                .contact_list_reader
                .lock()
                .map(|guard| Arc::clone(&*guard))
                .unwrap_or_else(|_| crate::slots::empty_contact_list_reader()),
            profile_lookup: self
                .profile_lookup
                .lock()
                .map(|guard| Arc::clone(&*guard))
                .unwrap_or_else(|_| crate::substrate::empty_profile_lookup()),
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
            gc_budget_ceiling: self.gc_budget_ceiling,
            user_agent: self.user_agent.lock().ok().and_then(|g| g.clone()),
            outbound_public_tags: self
                .outbound_public_tags
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .unwrap_or_default(),
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
    /// #1811 — crate-registered FTS scope registry (compiled + installed into
    /// the kernel store at `apply_to_kernel`).
    pub search_scope_registry: Arc<SearchScopeRegistry>,
    pub dm_inbox_relays: Arc<dyn DmInboxRelayLookup>,
    pub contact_list_reader: Arc<dyn ContactListReader>,
    pub profile_lookup: Arc<dyn ProfileLookup>,
    pub blocked_relays: Arc<dyn BlockedRelayLookup>,
    pub bootstrap_self_kinds: Option<Vec<u32>>,
    pub routing_substrate: Option<Arc<RoutingSubstrateFactory>>,
    pub publish_resolver: Option<Arc<PublishResolverFactory>>,
    pub external_event_sink_policy: Option<Arc<ExternalEventSinkPolicyFactory>>,
    pub kernel_clock: Option<Arc<dyn crate::kernel::Clock>>,
    /// Test-support only — see `ActorConfigSources::gc_budget_ceiling`.
    /// Always `None` in production builds.
    pub gc_budget_ceiling: Option<usize>,
    pub user_agent: Option<String>,
    pub outbound_public_tags: Vec<Vec<String>>,
}

impl ActorConfig {
    /// Construct the relay `Pool`, threading the app-configured relay
    /// User-Agent (Flow A) into the handshake. `None` → the transport's
    /// built-in `nmp/<ver>` fallback.
    #[must_use]
    pub fn build_pool(
        &self,
        events: impl nmp_network::pool::PoolEventSink,
    ) -> nmp_network::pool::Pool {
        nmp_network::pool::Pool::new(
            nmp_network::pool::PoolConfig {
                user_agent: self.user_agent.clone(),
                ..Default::default()
            },
            events,
        )
    }

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
                kernel.mailbox_cache_arc(),
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
        // #1811 — compile the crate-registered search scopes (noun-free
        // CompiledIndexSpec) and install them into the kernel's store. The
        // store now exists (kernel construction is complete); the registry runs
        // its opaque extractors + the shared tokenizer at ingest. A `Reset`
        // re-runs `apply_to_kernel`, so the fresh store is re-installed.
        self.search_scope_registry
            .install_into(&*kernel.event_store_handle());
        kernel.set_dm_inbox_relay_lookup(Arc::clone(&self.dm_inbox_relays));
        kernel.set_contact_list_reader(Arc::clone(&self.contact_list_reader));
        kernel.set_profile_lookup(Arc::clone(&self.profile_lookup));
        kernel.set_blocked_relay_lookup(Arc::clone(&self.blocked_relays));
        kernel.set_bootstrap_self_kinds_override(self.bootstrap_self_kinds.clone());
        kernel.set_outbound_public_tags(self.outbound_public_tags.clone());
        if let Some(hook) = &self.coverage_hook {
            kernel.lifecycle_mut().set_coverage_hook(Arc::clone(hook));
        }
        if let Some(interceptor) = &self.req_frame_interceptor {
            kernel
                .lifecycle_mut()
                .set_req_frame_interceptor(Arc::clone(interceptor));
        }
        // Test-support: install the configured GC budget ceiling (if any) so
        // `derive_store_gc_inputs` opts into durable LRU eviction for this
        // session. No-op in production builds.
        #[cfg(any(test, feature = "test-support"))]
        if let Some(ceiling) = self.gc_budget_ceiling {
            kernel.set_gc_budget_ceiling(ceiling);
        }
    }
}
