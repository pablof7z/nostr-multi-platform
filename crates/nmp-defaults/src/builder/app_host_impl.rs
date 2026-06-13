//! `impl<S> AppHost for NmpAppBuilder<S>` — the substrate-trait delegation
//! surface for the builder.
//!
//! Extracted as a cohesive child submodule of `builder` (ADR-0053 work) so the
//! composition-root file stays under the 500-LOC hard ceiling / its size
//! baseline (AGENTS.md file-size rule). Every method borrows the builder's
//! `NmpApp` and delegates to the inherent method of the same name.

use std::ops::Range;
use std::sync::Arc;

use nmp_ffi::NmpApp;

use super::*;

impl<S> AppHost for NmpAppBuilder<S> {
    fn register_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static,
    {
        // SAFETY: `self.app` non-null (builder invariant). Shared borrow via
        // `&self` is safe — all AppHost methods take `&self`.
        let app: &NmpApp = unsafe { &*self.app };
        app.register_snapshot_projection(key, f);
    }

    fn register_snapshot_projection_gated<K, F>(
        &self,
        key: K,
        gate: Arc<dyn nmp_core::ChangeGate>,
        f: F,
    ) where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static,
    {
        // SAFETY: `self.app` non-null (builder invariant). Shared borrow via
        // `&self` is safe — all AppHost methods take `&self`.
        let app: &NmpApp = unsafe { &*self.app };
        app.register_snapshot_projection_gated(key, gate, f);
    }

    fn register_typed_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    {
        // SAFETY: `self.app` non-null (builder invariant). Shared borrow via
        // `&self` is safe — all AppHost methods take `&self`.
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
        // SAFETY: `self.app` non-null (builder invariant). Shared borrow via
        // `&self` is safe — all AppHost methods take `&self`.
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
        // SAFETY: `self.app` non-null (builder invariant). Shared borrow via
        // `&self` is safe — all AppHost methods take `&self`.
        let app: &NmpApp = unsafe { &*self.app };
        NmpApp::declare_consumed_projections(app, keys);
    }

    fn set_coverage_hook(&self, hook: nmp_core::subs::PlanCoverageHook) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_coverage_hook(hook);
    }

    fn set_req_frame_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::ReqFrameInterceptor>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_req_frame_interceptor(interceptor);
    }

    fn add_relay_text_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) {
        let app: &NmpApp = unsafe { &*self.app };
        app.add_relay_text_interceptor(interceptor);
    }

    fn add_relay_connected_hook(&self, hook: Arc<dyn nmp_core::substrate::RelayConnectedHook>) {
        let app: &NmpApp = unsafe { &*self.app };
        app.add_relay_connected_hook(hook);
    }

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
        range: std::ops::Range<u32>,
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

    fn set_dm_inbox_relay_lookup(&self, lookup: Arc<dyn nmp_core::substrate::DmInboxRelayLookup>) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_dm_inbox_relay_lookup(lookup);
    }

    fn set_mailbox_cache_reader(&self, cache: Arc<dyn nmp_core::substrate::MailboxCache>) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_mailbox_cache_reader(cache);
    }

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
                Arc<dyn nmp_core::store::EventStore>,
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

    fn set_raw_event_forward_policy_factory<F>(&self, factory: F)
    where
        F: Fn(
                nmp_core::substrate::RawEventForwardPolicyContext,
            ) -> Vec<Arc<dyn nmp_core::substrate::RawEventForwardPolicy>>
            + Send
            + Sync
            + 'static,
    {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_raw_event_forward_policy_factory(factory);
    }

    fn active_local_keys(&self) -> nmp_core::slots::ActiveLocalKeysSlot {
        let app: &NmpApp = unsafe { &*self.app };
        app.active_local_keys()
    }

    fn active_pubkey(&self) -> nmp_core::slots::ActiveAccountSlot {
        let app: &NmpApp = unsafe { &*self.app };
        app.active_account_handle()
    }

    fn actor_sender(&self) -> nmp_core::CommandSender {
        let app: &NmpApp = unsafe { &*self.app };
        app.actor_sender()
    }

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

    fn register_raw_event_observer(
        &self,
        kinds: nmp_core::KindFilter,
        observer: Arc<dyn nmp_core::RawEventObserver>,
    ) -> nmp_core::RawEventObserverId {
        let app: &NmpApp = unsafe { &*self.app };
        app.register_raw_event_observer(kinds, observer)
    }

    fn unregister_raw_event_observer(&self, id: nmp_core::RawEventObserverId) {
        let app: &NmpApp = unsafe { &*self.app };
        app.unregister_raw_event_observer(id);
    }

    fn configured_relays_handle(&self) -> nmp_core::AppRelaySlot {
        let app: &NmpApp = unsafe { &*self.app };
        app.configured_relays_handle()
    }

    fn set_nostrconnect_bootstrap_relay(&self, url: String) {
        let app: &NmpApp = unsafe { &*self.app };
        app.set_nostrconnect_bootstrap_relay(url);
    }

    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        // SAFETY: `self.app` non-null (builder invariant). Shared borrow via
        // `&self` is safe — all AppHost methods take `&self`.
        let app: &NmpApp = unsafe { &*self.app };
        app.register_identity_change_observer(f);
    }
}
