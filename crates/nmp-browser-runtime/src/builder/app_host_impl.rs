//! `AppHost` narrow-trait implementations for `BrowserAppBuilder<S>`.
//!
//! Mirrors `explicit composition/src/builder/app_host_impl.rs` but delegates through
//! `self.lock()` (a `Mutex<BrowserBuilderInner>`) rather than an `unsafe`
//! raw-pointer dereference. The interior-mutability model is the same:
//! `&self` trait impls accumulate state that is applied to the kernel in
//! `start()`.
//!
//! # Doctrine
//!
//! * **D0** — only substrate-level types cross this surface; no NIP nouns.
//! * **D6** — poisoned mutex means the registration is silently dropped; the
//!   builder never panics.
//! * **D8** — all methods are lock-and-insert; no I/O, no blocking.

use std::ops::Range;
use std::sync::Arc;

use nmp_core::substrate::{ActionRegistrar, PreferredRelaySource};
use nmp_core::substrate::{
    BlockedRelayLookupRegistrar, ConfiguredRelaysChangeRegistrar, ContactsLookup,
    CoverageHookRegistrar, DmInboxRelayRegistrar, HostCapabilities, IdentityChangeRegistrar,
    IncrementalApplyError, IngestParserRegistrar, InputScopeRegistrar, KernelReaderRegistrar,
    ObservedProjection, ObservedProjectionRegistrar, RelayConnectedHookRegistrar,
    RelayTextInterceptorRegistrar, ReqFrameInterceptorRegistrar, RoutingFactoryRegistrar,
    SearchScopeRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::{AppRelaySlot, CommandSender, ObservedProjectionId, TypedProjectionData};

use super::BrowserAppBuilder;

// ── Helper: lock or return early ─────────────────────────────────────────────
//
// All registrar impls use this macro-less helper pattern:
// `let Ok(mut g) = self.inner.lock() else { return <default>; };`
// The match arm can't be a macro because different methods return different
// default types.

// ── SnapshotProjectionRegistrar ───────────────────────────────────────────────

impl<S> SnapshotProjectionRegistrar for BrowserAppBuilder<S> {
    fn register_typed_snapshot_projection<K, F>(&self, key: K, f: F)
    where
        K: Into<String>,
        F: Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    {
        let Ok(g) = self.inner.lock() else { return };
        g.reducer.register_typed_snapshot_projection(key, f);
    }

    fn declare_consumed_projections<I, K>(&self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        let Ok(g) = self.inner.lock() else { return };
        g.reducer.declare_consumed_projections(keys);
    }

    fn declare_incremental_apply(&self) -> Result<(), IncrementalApplyError> {
        let Ok(g) = self.inner.lock() else {
            return Err(IncrementalApplyError::RegistryUnavailable);
        };
        g.reducer.declare_incremental_apply()
    }

    fn incremental_apply_handle(&self) -> Arc<std::sync::atomic::AtomicBool> {
        let Ok(g) = self.inner.lock() else {
            return Arc::new(std::sync::atomic::AtomicBool::new(false));
        };
        g.reducer.incremental_apply_handle()
    }

    fn frame_identity_handles(
        &self,
    ) -> (
        Arc<std::sync::atomic::AtomicU64>,
        Arc<std::sync::atomic::AtomicU64>,
    ) {
        let Ok(g) = self.inner.lock() else {
            return (
                Arc::new(std::sync::atomic::AtomicU64::new(0)),
                Arc::new(std::sync::atomic::AtomicU64::new(0)),
            );
        };
        g.reducer.frame_identity_handles()
    }

    fn remove_snapshot_projection(&self, key: &str) {
        let Ok(g) = self.inner.lock() else { return };
        g.reducer.remove_snapshot_projection(key);
    }
}

// ── IngestParserRegistrar ────────────────────────────────────────────────────

impl<S> IngestParserRegistrar for BrowserAppBuilder<S> {
    fn register_ingest_parser(
        &self,
        kind: u32,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) {
        let Ok(g) = self.inner.lock() else { return };
        g.reducer.register_ingest_parser(kind, parser);
    }

    fn replace_ingest_parser(
        &self,
        kind: u32,
        slot_key: &'static str,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> Option<Arc<dyn nmp_core::substrate::IngestParser>> {
        let Ok(g) = self.inner.lock() else {
            return None;
        };
        g.reducer.replace_ingest_parser(kind, slot_key, parser)
    }

    fn unregister_ingest_parser(&self, kind: u32, slot_key: &'static str) {
        let Ok(g) = self.inner.lock() else { return };
        g.reducer.unregister_ingest_parser(kind, slot_key);
    }

    fn replace_ingest_parser_range(
        &self,
        range: Range<u32>,
        slot_key: &'static str,
        parser: Arc<dyn nmp_core::substrate::IngestParser>,
    ) -> Option<Arc<dyn nmp_core::substrate::IngestParser>> {
        let Ok(g) = self.inner.lock() else {
            return None;
        };
        g.reducer
            .replace_ingest_parser_range(range, slot_key, parser)
    }

    fn unregister_ingest_parser_range(&self, slot_key: &'static str) {
        let Ok(g) = self.inner.lock() else { return };
        g.reducer.unregister_ingest_parser_range(slot_key);
    }
}

// ── ObservedProjectionRegistrar ─────────────────────────────────────────────

struct NoopObservedProjectionRegistrar;

impl ObservedProjectionRegistrar for NoopObservedProjectionRegistrar {
    fn open_observed_projection(&self, _decl: ObservedProjection) -> ObservedProjectionId {
        ObservedProjectionId(0)
    }

    fn close_observed_projection(&self, _id: ObservedProjectionId) {}

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        Arc::new(Self)
    }
}

impl<S> ObservedProjectionRegistrar for BrowserAppBuilder<S> {
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId {
        let Ok(mut g) = self.inner.lock() else {
            return ObservedProjectionId(0);
        };
        g.reducer.open_observed_projection(decl)
    }

    fn close_observed_projection(&self, id: ObservedProjectionId) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.reducer.close_observed_projection(id);
    }

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        let Ok(g) = self.inner.lock() else {
            return Arc::new(NoopObservedProjectionRegistrar);
        };
        Arc::new(g.reducer.observed_projection_command_handle(
            Arc::clone(&g.observed_projection_sessions),
            CommandSender::new_bounded(g.inbox_tx.clone()),
        ))
    }
}

// ── IdentityChangeRegistrar ───────────────────────────────────────────────────

impl<S> IdentityChangeRegistrar for BrowserAppBuilder<S> {
    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        let Ok(mut g) = self.inner.lock() else { return };
        g.identity_change_observers.push(Box::new(f));
    }
}

impl<S> ConfiguredRelaysChangeRegistrar for BrowserAppBuilder<S> {
    fn register_configured_relays_change_observer<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let Ok(mut g) = self.inner.lock() else { return };
        g.configured_relays_change_observers.push(Box::new(f));
    }
}

// ── ReqFrameInterceptorRegistrar ─────────────────────────────────────────────

impl<S> ReqFrameInterceptorRegistrar for BrowserAppBuilder<S> {
    fn set_req_frame_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::ReqFrameInterceptor>,
    ) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.req_frame_interceptor = Some(interceptor);
    }
}

// ── RelayTextInterceptorRegistrar ────────────────────────────────────────────

impl<S> RelayTextInterceptorRegistrar for BrowserAppBuilder<S> {
    fn add_relay_text_interceptor(
        &self,
        interceptor: Arc<dyn nmp_core::substrate::RelayTextInterceptor>,
    ) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.relay_text_interceptors.push(interceptor);
    }
}

// ── RelayConnectedHookRegistrar ───────────────────────────────────────────────

impl<S> RelayConnectedHookRegistrar for BrowserAppBuilder<S> {
    fn add_relay_connected_hook(&self, hook: Arc<dyn nmp_core::substrate::RelayConnectedHook>) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.relay_connected_hooks.push(hook);
    }
}

// ── CoverageHookRegistrar ─────────────────────────────────────────────────────

impl<S> CoverageHookRegistrar for BrowserAppBuilder<S> {
    fn set_coverage_hook(&self, hook: nmp_core::subs::PlanCoverageHook) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.coverage_hook = Some(hook);
    }
}

// ── KernelReaderRegistrar ─────────────────────────────────────────────────────

impl<S> KernelReaderRegistrar for BrowserAppBuilder<S> {
    fn set_profile_lookup(&self, lookup: Arc<dyn nmp_core::substrate::ProfileLookup>) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.profile_lookup = Some(lookup);
    }

    fn set_contacts_lookup(&self, lookup: Arc<dyn ContactsLookup>) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.contacts_lookup = Some(lookup);
    }

    fn set_mailbox_cache_reader(&self, cache: Arc<dyn nmp_core::substrate::MailboxCache>) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.mailbox_cache_reader = Some(cache);
    }
}

// ── DmInboxRelayRegistrar ────────────────────────────────────────────────────

impl<S> DmInboxRelayRegistrar for BrowserAppBuilder<S> {
    fn set_dm_inbox_relay_lookup(&self, lookup: Arc<dyn nmp_core::substrate::DmInboxRelayLookup>) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.dm_inbox_relay_lookup = Some(lookup);
    }
}

// ── BlockedRelayLookupRegistrar ───────────────────────────────────────────────

impl<S> BlockedRelayLookupRegistrar for BrowserAppBuilder<S> {
    fn set_blocked_relay_lookup(&self, lookup: Arc<dyn nmp_core::substrate::BlockedRelayLookup>) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.blocked_relay_lookup = Some(lookup);
    }
}

// ── RoutingFactoryRegistrar ───────────────────────────────────────────────────

impl<S> RoutingFactoryRegistrar for BrowserAppBuilder<S> {
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
        let Ok(mut g) = self.inner.lock() else { return };
        g.routing_substrate_factory = Some(Box::new(factory));
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
        let Ok(mut g) = self.inner.lock() else { return };
        g.publish_resolver_factory = Some(Box::new(factory));
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
        let Ok(mut g) = self.inner.lock() else { return };
        g.external_event_sink_policy_factory = Some(Box::new(factory));
    }

    fn set_nostrconnect_bootstrap_relay(&self, url: String) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.nostrconnect_bootstrap_relay = Some(url);
    }

    fn set_nostrconnect_perms(&self, perms: String) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.nostrconnect_perms = Some(perms);
    }

    fn set_relay_user_agent(&self, user_agent: String) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.relay_user_agent = Some(user_agent);
    }

    fn set_outbound_public_tags(&self, tags: Vec<Vec<String>>) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.outbound_public_tags = tags;
    }
}

// ── SearchScopeRegistrar ──────────────────────────────────────────────────────

impl<S> SearchScopeRegistrar for BrowserAppBuilder<S> {
    fn register_search_scope(&self, provider: Arc<dyn nmp_core::substrate::SearchScopeProvider>) {
        // Delegates to the shared `SearchScopeRegistry` directly (the registry
        // is itself a `SearchScopeRegistrar`; install_into is called at start()).
        let Ok(g) = self.inner.lock() else { return };
        g.search_scope_registry.register_search_scope(provider);
    }
}

// ── InputScopeRegistrar ───────────────────────────────────────────────────────

impl<S> InputScopeRegistrar for BrowserAppBuilder<S> {
    fn register_input_scope(&self, recognizer: Arc<dyn nmp_core::substrate::InputScopeRecognizer>) {
        let Ok(g) = self.inner.lock() else { return };
        g.input_scope_registry.register_input_scope(recognizer);
    }
}

// ── HostCapabilities ──────────────────────────────────────────────────────────

impl<S> HostCapabilities for BrowserAppBuilder<S> {
    fn active_pubkey(&self) -> nmp_core::slots::ActiveAccountSlot {
        let Ok(g) = self.inner.lock() else {
            return Arc::new(std::sync::Mutex::new(None));
        };
        g.reducer.active_account_handle()
    }

    fn actor_sender(&self) -> CommandSender {
        let Ok(g) = self.inner.lock() else {
            // D6: return a sender that will always fail to send (detached channel).
            let (tx, rx) = std::sync::mpsc::sync_channel::<nmp_core::actor::ActorMail>(0);
            drop(rx);
            return CommandSender::new_bounded(tx);
        };
        CommandSender::new_bounded(g.inbox_tx.clone())
    }

    fn configured_relays_handle(&self) -> AppRelaySlot {
        let Ok(g) = self.inner.lock() else {
            return Arc::new(std::sync::Mutex::new(nmp_core::AppRelayList::default()));
        };
        Arc::clone(&g.configured_relays_slot)
    }

    fn install_preferred_relay_source(&self, source: Arc<dyn PreferredRelaySource>) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.preferred_relay_source = Some(source);
    }
}

// ── CapabilityProvider registrar (inherent, not a trait) ─────────────────────

impl<S> BrowserAppBuilder<S> {
    /// Register one or more capability/signer providers, keyed by their
    /// `Signer::pubkey()` (ADR-0067 §10a, single-door per account).
    ///
    /// Available in ALL builder states. Providers accumulate into the inner
    /// state and are moved into the `CapabilityProviderRegistry` at `start()`.
    /// Multiple providers for the same pubkey are last-write-wins.
    ///
    /// Supported providers for direct builder injection (#2049/#2066/#2067):
    /// - `nmp_signers::LocalKeySigner` — synchronous in-memory signing.
    /// - `nmp_signers::Nip07Signer` — async via browser extension; active on
    ///   `wasm32 + feature = "wasm"` builds only. On native builds the
    ///   provider is unresolvable and the runtime emits `SignRequest` for
    ///   host-brokering instead.
    ///
    /// Browser NIP-46 bunker sign-in is started through `set_identity
    /// kind=nip46` so the runtime can own the handshake lifecycle.
    pub fn with_capability_providers(
        &self,
        providers: impl IntoIterator<Item = std::sync::Arc<dyn nmp_signers::Signer>>,
    ) {
        let Ok(mut g) = self.inner.lock() else { return };
        g.capability_providers.extend(providers);
    }
}

// ── ActionRegistrar ───────────────────────────────────────────────────────────
//
// Takes `&mut self` (by the trait contract) — the `Mutex` unlock + `&mut`
// on the `ActionRegistry` is sound because the builder owns it exclusively
// during the composition phase.

impl<S> ActionRegistrar for BrowserAppBuilder<S> {
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        let Ok(mut g) = self.inner.lock() else {
            return Ok(());
        };
        g.action_registry.register_action(module)
    }

    fn register_default_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> bool {
        let Ok(mut g) = self.inner.lock() else {
            return false;
        };
        g.action_registry.register_default_action(module)
    }
}
