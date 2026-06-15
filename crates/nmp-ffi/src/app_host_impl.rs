//! `impl AppHost for NmpApp` — the substrate-trait delegation surface.
//!
//! Extracted from `lib.rs` (ADR-0053 work) to keep that file within its size
//! baseline. Every method delegates to the inherent `NmpApp` method of the same
//! name; the trait exists so reusable protocol/composition crates can wire
//! against `&impl AppHost` without naming the concrete C-ABI `NmpApp` type.

use std::ops::Range;
use std::sync::Arc;

use super::*;

impl nmp_core::substrate::AppHost for NmpApp {
    fn register_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> serde_json::Value + Send + Sync + 'static,
    {
        NmpApp::register_snapshot_projection(self, key, f);
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
        NmpApp::register_snapshot_projection_gated(self, key, gate, f);
    }

    fn register_typed_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    {
        NmpApp::register_typed_snapshot_projection(self, key, f);
    }

    fn register_snapshot_tick_observer<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        NmpApp::register_snapshot_tick_observer(self, f);
    }

    fn declare_consumed_projections<I, K>(&self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        NmpApp::declare_consumed_projections(self, keys);
    }

    fn declare_incremental_apply(&self) -> Result<(), nmp_core::substrate::IncrementalApplyError> {
        NmpApp::declare_incremental_apply(self)
    }

    fn incremental_apply_handle(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        NmpApp::incremental_apply_handle(self)
    }

    fn frame_identity_handles(
        &self,
    ) -> (
        std::sync::Arc<std::sync::atomic::AtomicU64>,
        std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) {
        NmpApp::frame_identity_handles(self)
    }

    fn set_coverage_hook(&self, hook: nmp_core::subs::PlanCoverageHook) {
        NmpApp::set_coverage_hook(self, hook);
    }

    fn set_req_frame_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::ReqFrameInterceptor>,
    ) {
        NmpApp::set_req_frame_interceptor(self, interceptor);
    }

    fn add_relay_text_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) {
        NmpApp::add_relay_text_interceptor(self, interceptor);
    }

    fn add_relay_connected_hook(&self, hook: Arc<dyn nmp_core::substrate::RelayConnectedHook>) {
        NmpApp::add_relay_connected_hook(self, hook);
    }

    fn register_ingest_parser(
        &self,
        kind: u32,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) {
        NmpApp::register_ingest_parser(self, kind, parser);
    }

    fn replace_ingest_parser(
        &self,
        kind: u32,
        slot_key: &'static str,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> Option<Arc<dyn nmp_core::substrate::IngestParser>> {
        NmpApp::replace_ingest_parser(self, kind, slot_key, parser)
    }

    fn unregister_ingest_parser(&self, kind: u32, slot_key: &'static str) {
        NmpApp::unregister_ingest_parser(self, kind, slot_key);
    }

    fn replace_ingest_parser_range(
        &self,
        range: std::ops::Range<u32>,
        slot_key: &'static str,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> Option<Arc<dyn nmp_core::substrate::IngestParser>> {
        NmpApp::replace_ingest_parser_range(self, range, slot_key, parser)
    }

    fn unregister_ingest_parser_range(&self, slot_key: &'static str) {
        NmpApp::unregister_ingest_parser_range(self, slot_key);
    }

    fn set_dm_inbox_relay_lookup(&self, lookup: Arc<dyn nmp_core::substrate::DmInboxRelayLookup>) {
        NmpApp::set_dm_inbox_relay_lookup(self, lookup);
    }

    fn set_profile_lookup(&self, lookup: Arc<dyn nmp_core::substrate::ProfileLookup>) {
        NmpApp::set_profile_lookup(self, lookup);
    }

    fn set_contacts_lookup(&self, lookup: Arc<dyn nmp_core::substrate::ContactsLookup>) {
        NmpApp::set_contacts_lookup(self, lookup);
    }

    fn set_mailbox_cache_reader(&self, cache: Arc<dyn nmp_core::substrate::MailboxCache>) {
        NmpApp::set_mailbox_cache_reader(self, cache);
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
        NmpApp::set_routing_substrate(self, factory);
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
        NmpApp::set_publish_resolver_factory(self, factory);
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
        NmpApp::set_raw_event_forward_policy_factory(self, factory);
    }

    fn active_local_keys(&self) -> nmp_core::slots::ActiveLocalKeysSlot {
        NmpApp::active_local_keys(self)
    }

    fn active_pubkey(&self) -> nmp_core::slots::ActiveAccountSlot {
        NmpApp::active_account_handle(self)
    }

    fn actor_sender(&self) -> nmp_core::CommandSender {
        NmpApp::actor_sender(self)
    }

    fn register_event_observer(
        &self,
        observer: Arc<dyn KernelEventObserver>,
    ) -> KernelEventObserverId {
        NmpApp::register_event_observer(self, observer)
    }

    fn unregister_event_observer(&self, id: KernelEventObserverId) {
        NmpApp::unregister_event_observer(self, id);
    }

    fn swap_singleton_event_observer(
        &self,
        new: Option<KernelEventObserverId>,
    ) -> Option<KernelEventObserverId> {
        NmpApp::swap_singleton_event_observer(self, new)
    }

    fn register_raw_event_observer(
        &self,
        kinds: KindFilter,
        observer: Arc<dyn RawEventObserver>,
    ) -> RawEventObserverId {
        NmpApp::register_raw_event_observer(self, kinds, observer)
    }

    fn unregister_raw_event_observer(&self, id: RawEventObserverId) {
        NmpApp::unregister_raw_event_observer(self, id);
    }

    fn configured_relays_handle(&self) -> nmp_core::AppRelaySlot {
        NmpApp::configured_relays_handle(self)
    }

    fn set_nostrconnect_bootstrap_relay(&self, url: String) {
        NmpApp::set_nostrconnect_bootstrap_relay(self, url)
    }

    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        NmpApp::register_identity_change_observer(self, f);
    }
}
