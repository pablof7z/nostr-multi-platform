//! Narrow host-trait delegation surface for `NmpApp`.
//!
//! Extracted from `lib.rs` (ADR-0070 work) to keep that file within its size
//! baseline. Every method delegates to the inherent `NmpApp` method of the same
//! name; the traits exist so reusable protocol/composition crates can wire
//! against `&impl <NarrowTrait>` without naming the concrete C-ABI `NmpApp`
//! type.
//!
//! D6: `NmpApp` implements each narrow registration/capability trait
//! individually and gets [`nmp_core::substrate::AppHost`] for free via the
//! blanket super-trait impl.

use std::ops::Range;
use std::sync::Arc;

use nmp_core::substrate::{
    BlockedRelayLookupRegistrar, ConfiguredRelaysChangeRegistrar, CoverageHookRegistrar,
    DmInboxRelayRegistrar, ExternalIdValidatorRegistrar, HostCapabilities, IdentityChangeRegistrar,
    IngestParserRegistrar, InputScopeRegistrar, KernelReaderRegistrar, RelayConnectedHookRegistrar,
    RelayTextInterceptorRegistrar, ReqFrameInterceptorRegistrar, RoutingFactoryRegistrar,
    SearchScopeRegistrar, SnapshotProjectionRegistrar,
};
use nmp_ownership::ProjectionRegistrationKey;

use super::*;

impl SnapshotProjectionRegistrar for NmpApp {
    fn register_typed_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<ProjectionRegistrationKey>,
        F: Fn() -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    {
        NmpApp::register_typed_snapshot_projection(self, key, f);
    }

    fn register_typed_snapshot_projection_with_time<K, F>(&self, key: K, f: F)
    where
        K: Into<ProjectionRegistrationKey>,
        F: Fn(u64) -> Option<nmp_core::TypedProjectionData> + Send + Sync + 'static,
    {
        NmpApp::register_typed_snapshot_projection_with_time(self, key, f);
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

    fn remove_snapshot_projection(&self, key: &str) {
        NmpApp::remove_snapshot_projection(self, key);
    }
}

impl CoverageHookRegistrar for NmpApp {
    fn set_coverage_hook(&self, hook: nmp_core::subs::PlanCoverageHook) {
        NmpApp::set_coverage_hook(self, hook);
    }
}

impl ReqFrameInterceptorRegistrar for NmpApp {
    fn set_req_frame_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::ReqFrameInterceptor>,
    ) {
        NmpApp::set_req_frame_interceptor(self, interceptor);
    }
}

impl RelayTextInterceptorRegistrar for NmpApp {
    fn add_relay_text_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) {
        NmpApp::add_relay_text_interceptor(self, interceptor);
    }
}

impl RelayConnectedHookRegistrar for NmpApp {
    fn add_relay_connected_hook(&self, hook: Arc<dyn nmp_core::substrate::RelayConnectedHook>) {
        NmpApp::add_relay_connected_hook(self, hook);
    }
}

impl IngestParserRegistrar for NmpApp {
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
        range: Range<u32>,
        slot_key: &'static str,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> Option<Arc<dyn nmp_core::substrate::IngestParser>> {
        NmpApp::replace_ingest_parser_range(self, range, slot_key, parser)
    }

    fn unregister_ingest_parser_range(&self, slot_key: &'static str) {
        NmpApp::unregister_ingest_parser_range(self, slot_key);
    }
}

impl SearchScopeRegistrar for NmpApp {
    fn register_search_scope(&self, provider: Arc<dyn nmp_core::substrate::SearchScopeProvider>) {
        NmpApp::register_search_scope(self, provider);
    }
}

impl InputScopeRegistrar for NmpApp {
    fn register_input_scope(&self, recognizer: Arc<dyn nmp_core::substrate::InputScopeRecognizer>) {
        NmpApp::register_input_scope(self, recognizer);
    }
}

impl DmInboxRelayRegistrar for NmpApp {
    fn set_dm_inbox_relay_lookup(&self, lookup: Arc<dyn nmp_core::substrate::DmInboxRelayLookup>) {
        NmpApp::set_dm_inbox_relay_lookup(self, lookup);
    }
}

impl BlockedRelayLookupRegistrar for NmpApp {
    fn set_blocked_relay_lookup(&self, lookup: Arc<dyn nmp_core::substrate::BlockedRelayLookup>) {
        NmpApp::set_blocked_relay_lookup(self, lookup);
    }
}

impl ExternalIdValidatorRegistrar for NmpApp {
    fn set_external_id_validator(
        &self,
        validator: Arc<dyn nmp_core::substrate::ExternalIdValidator>,
    ) {
        NmpApp::set_external_id_validator(self, validator);
    }
}

impl KernelReaderRegistrar for NmpApp {
    fn set_profile_lookup(&self, lookup: Arc<dyn nmp_core::substrate::ProfileLookup>) {
        NmpApp::set_profile_lookup(self, lookup);
    }

    fn set_mailbox_cache_reader(&self, cache: Arc<dyn nmp_core::substrate::MailboxCache>) {
        NmpApp::set_mailbox_cache_reader(self, cache);
    }

    fn set_contact_list_reader(&self, reader: Arc<dyn nmp_core::slots::ContactListReader>) {
        NmpApp::set_contact_list_reader(self, reader);
    }
}

impl RoutingFactoryRegistrar for NmpApp {
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
                Arc<dyn nmp_store::EventStore>,
                Arc<dyn nmp_core::substrate::MailboxCache>,
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

    fn set_external_event_sink_policy_factory<F>(&self, factory: F)
    where
        F: Fn(
                nmp_core::substrate::RawEventForwardPolicyContext,
            ) -> Vec<Arc<dyn nmp_core::substrate::ExternalEventSinkPolicy>>
            + Send
            + Sync
            + 'static,
    {
        NmpApp::set_external_event_sink_policy_factory(self, factory);
    }

    fn set_nostrconnect_bootstrap_relay(&self, url: String) {
        NmpApp::set_nostrconnect_bootstrap_relay(self, url);
    }

    fn set_nostrconnect_perms(&self, perms: String) {
        NmpApp::set_nostrconnect_perms(self, perms);
    }

    fn set_relay_user_agent(&self, user_agent: String) {
        let _ = NmpApp::set_relay_user_agent(self, user_agent);
    }

    fn set_outbound_public_tags(&self, tags: Vec<Vec<String>>) {
        let _ = NmpApp::set_outbound_public_tags(self, tags);
    }
}

impl HostCapabilities for NmpApp {
    fn active_pubkey(&self) -> nmp_core::slots::ActiveAccountSlot {
        NmpApp::active_account_handle(self)
    }

    fn actor_sender(&self) -> nmp_core::CommandSender {
        NmpApp::actor_sender(self)
    }

    fn configured_relays_handle(&self) -> nmp_core::AppRelaySlot {
        NmpApp::configured_relays_handle(self)
    }

    fn event_store_handle(&self) -> nmp_core::slots::EventStoreSlot {
        NmpApp::event_store_handle(self)
    }

    /// Store the host-installed preferred-relay source (NIP-50 search reads it
    /// back in `open_search`). Overrides the trait's no-op default only when
    /// the search concept is composed.
    #[cfg(feature = "search")]
    fn install_preferred_relay_source(
        &self,
        source: std::sync::Arc<dyn nmp_core::substrate::PreferredRelaySource>,
    ) {
        NmpApp::install_preferred_relay_source(self, source);
    }
}

impl IdentityChangeRegistrar for NmpApp {
    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        NmpApp::register_identity_change_observer(self, f);
    }
}

impl ConfiguredRelaysChangeRegistrar for NmpApp {
    fn register_configured_relays_change_observer<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        NmpApp::register_configured_relays_change_observer(self, f);
    }
}
