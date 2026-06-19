//! Narrow host-trait delegation surface for `NmpAppBuilder<S>`.
//!
//! Extracted as a cohesive child submodule of `builder` (ADR-0053 work) so the
//! composition-root file stays under the 500-LOC hard ceiling / its size
//! baseline (AGENTS.md file-size rule). Every method borrows the builder's
//! `NmpApp` and delegates to the inherent method of the same name.
//!
//! D6: the builder implements each narrow registration/capability trait
//! individually and gets [`nmp_core::substrate::AppHost`] for free via the
//! blanket super-trait impl.

use std::ops::Range;
use std::sync::Arc;

use nmp_core::substrate::{
    BlockedRelayLookupRegistrar, CoverageHookRegistrar, DmInboxRelayRegistrar,
    EventObserverRegistrar, HostCapabilities, IdentityChangeRegistrar, IngestParserRegistrar,
    KernelReaderRegistrar, RelayConnectedHookRegistrar, RelayTextInterceptorRegistrar,
    ReqFrameInterceptorRegistrar, RoutingFactoryRegistrar, SnapshotProjectionRegistrar,
};
use nmp_ffi::NmpApp;

use super::*;

impl<S> SnapshotProjectionRegistrar for NmpAppBuilder<S> {
    fn register_typed_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    {
        let app: &NmpApp = unsafe { &*self.app };
        // Forward into the same shared registry the generic projection seam
        // writes to (ADR-0037 Commitment 4: typed + generic share the key
        // space). Fully qualified to the inherent `NmpApp` method.
        NmpApp::register_typed_snapshot_projection(app, key, f);
    }

    fn register_snapshot_tick_observer<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let app: &NmpApp = unsafe { &*self.app };
        // Forwards into the same shared registry the projection seams write to;
        // tick observers live alongside the projection closures (one slot, bound
        // onto the kernel and surviving `Reset`). Fully qualified to the
        // inherent `NmpApp` method.
        NmpApp::register_snapshot_tick_observer(app, f);
    }

    fn declare_consumed_projections<I, K>(&self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        let app: &NmpApp = unsafe { &*self.app };
        NmpApp::declare_consumed_projections(app, keys);
    }

    fn declare_incremental_apply(&self) -> Result<(), nmp_core::substrate::IncrementalApplyError> {
        let app: &NmpApp = unsafe { &*self.app };
        NmpApp::declare_incremental_apply(app)
    }

    fn incremental_apply_handle(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        let app: &NmpApp = unsafe { &*self.app };
        app.incremental_apply_handle()
    }

    fn frame_identity_handles(
        &self,
    ) -> (
        std::sync::Arc<std::sync::atomic::AtomicU64>,
        std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.frame_identity_handles()
    }
}

impl<S> CoverageHookRegistrar for NmpAppBuilder<S> {
    fn set_coverage_hook(&self, hook: nmp_core::subs::PlanCoverageHook) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_coverage_hook(hook);
    }
}

impl<S> ReqFrameInterceptorRegistrar for NmpAppBuilder<S> {
    fn set_req_frame_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::ReqFrameInterceptor>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_req_frame_interceptor(interceptor);
    }
}

impl<S> RelayTextInterceptorRegistrar for NmpAppBuilder<S> {
    fn add_relay_text_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.add_relay_text_interceptor(interceptor);
    }
}

impl<S> RelayConnectedHookRegistrar for NmpAppBuilder<S> {
    fn add_relay_connected_hook(&self, hook: Arc<dyn nmp_core::substrate::RelayConnectedHook>) {
        let app: &NmpApp = unsafe { &*self.app };
        app.add_relay_connected_hook(hook);
    }
}

impl<S> IngestParserRegistrar for NmpAppBuilder<S> {
    fn register_ingest_parser(
        &self,
        kind: u32,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.register_ingest_parser(kind, parser);
    }

    fn replace_ingest_parser(
        &self,
        kind: u32,
        slot_key: &'static str,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> Option<Arc<dyn nmp_core::substrate::IngestParser>> {
        let app: &NmpApp = unsafe { &*self.app };
        app.replace_ingest_parser(kind, slot_key, parser)
    }

    fn unregister_ingest_parser(&self, kind: u32, slot_key: &'static str) {
        let app: &NmpApp = unsafe { &*self.app };
        app.unregister_ingest_parser(kind, slot_key);
    }

    fn replace_ingest_parser_range(
        &self,
        range: Range<u32>,
        slot_key: &'static str,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> Option<Arc<dyn nmp_core::substrate::IngestParser>> {
        let app: &NmpApp = unsafe { &*self.app };
        app.replace_ingest_parser_range(range, slot_key, parser)
    }

    fn unregister_ingest_parser_range(&self, slot_key: &'static str) {
        let app: &NmpApp = unsafe { &*self.app };
        app.unregister_ingest_parser_range(slot_key);
    }
}

impl<S> DmInboxRelayRegistrar for NmpAppBuilder<S> {
    fn set_dm_inbox_relay_lookup(&self, lookup: Arc<dyn nmp_core::substrate::DmInboxRelayLookup>) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_dm_inbox_relay_lookup(lookup);
    }
}

impl<S> BlockedRelayLookupRegistrar for NmpAppBuilder<S> {
    fn set_blocked_relay_lookup(&self, lookup: Arc<dyn nmp_core::substrate::BlockedRelayLookup>) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_blocked_relay_lookup(lookup);
    }
}

impl<S> KernelReaderRegistrar for NmpAppBuilder<S> {
    fn set_profile_lookup(&self, lookup: Arc<dyn nmp_core::substrate::ProfileLookup>) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_profile_lookup(lookup);
    }

    fn set_contacts_lookup(&self, lookup: Arc<dyn nmp_core::substrate::ContactsLookup>) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_contacts_lookup(lookup);
    }

    fn set_mailbox_cache_reader(&self, cache: Arc<dyn nmp_core::substrate::MailboxCache>) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_mailbox_cache_reader(cache);
    }
}

impl<S> RoutingFactoryRegistrar for NmpAppBuilder<S> {
    fn set_routing_substrate<F>(&self, factory: F)
    where
        F: Fn(
                Arc<dyn nmp_core::substrate::RoutingTraceObserver>,
            ) -> (
                Arc<dyn nmp_core::substrate::OutboxRouter>,
                Arc<dyn nmp_core::substrate::MailboxCache>,
            ) + Send
            + Sync
            + 'static,
    {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_routing_substrate(factory);
    }

    fn set_publish_resolver_factory<F>(&self, factory: F)
    where
        F: Fn(
                Arc<dyn nmp_store::EventStore>,
                nmp_core::slots::IndexerRelaysSlot,
                nmp_core::slots::LocalWriteRelaysSlot,
                nmp_core::slots::ActiveAccountSlot,
            ) -> Arc<dyn nmp_core::publish::OutboxResolver>
            + Send
            + Sync
            + 'static,
    {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_publish_resolver_factory(factory);
    }

    fn set_external_event_sink_policy_factory<F>(&self, factory: F)
    where
        F: Fn(
                nmp_core::substrate::RawEventForwardPolicyContext,
            ) -> Vec<Arc<dyn nmp_core::substrate::ExternalEventSinkPolicy>>
            + Send
            + Sync
            + 'static,
    {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_external_event_sink_policy_factory(factory);
    }

    fn set_nostrconnect_bootstrap_relay(&self, url: String) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_nostrconnect_bootstrap_relay(url);
    }

    fn set_nostrconnect_perms(&self, perms: String) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_nostrconnect_perms(perms);
    }
}

impl<S> HostCapabilities for NmpAppBuilder<S> {
    fn active_pubkey(&self) -> nmp_core::slots::ActiveAccountSlot {
        let app: &NmpApp = unsafe { &*self.app };
        app.active_account_handle()
    }

    fn actor_sender(&self) -> nmp_core::CommandSender {
        let app: &NmpApp = unsafe { &*self.app };
        app.actor_sender()
    }

    fn configured_relays_handle(&self) -> nmp_core::AppRelaySlot {
        let app: &NmpApp = unsafe { &*self.app };
        app.configured_relays_handle()
    }
}

impl<S> EventObserverRegistrar for NmpAppBuilder<S> {
    fn register_event_observer(
        &self,
        observer: Arc<dyn nmp_core::KernelEventObserver>,
    ) -> nmp_core::KernelEventObserverId {
        let app: &NmpApp = unsafe { &*self.app };
        app.register_event_observer(observer)
    }

    fn unregister_event_observer(&self, id: nmp_core::KernelEventObserverId) {
        let app: &NmpApp = unsafe { &*self.app };
        app.unregister_event_observer(id);
    }

    fn swap_singleton_event_observer(
        &self,
        new: Option<nmp_core::KernelEventObserverId>,
    ) -> Option<nmp_core::KernelEventObserverId> {
        let app: &NmpApp = unsafe { &*self.app };
        app.swap_singleton_event_observer(new)
    }

}

impl<S> IdentityChangeRegistrar for NmpAppBuilder<S> {
    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        let app: &NmpApp = unsafe { &*self.app };
        app.register_identity_change_observer(f);
    }
}
