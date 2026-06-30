//! Owned NMP substrate construction.
//!
//! This crate owns the shared construction that must not be hand-copied by app
//! roots: one mailbox cache shared by the NIP-65 reader, router factory, and
//! kind:10002 parser; one profile cache shared by the kernel profile reader and
//! kind:0 parser; one contacts cache shared by the kernel contacts reader and
//! kind:3 parser; plus the routing, publish resolver, raw forwarding, coverage,
//! NIP-77, blocked-relay, and native NIP-11 hooks that make the kernel routable.
//!
//! Protocol and app features are deliberately not installed here. App/runtime
//! roots compose those owner crates explicitly after this substrate floor.

use std::sync::Arc;

use nmp_core::publish::OutboxResolver;
use nmp_core::slots::{ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot};
use nmp_core::substrate::{
    ActionRegistrar, BlockedRelayLookupRegistrar, ContactsLookup, CoverageHookRegistrar,
    ExternalEventSinkPolicy, IngestParser, IngestParserRegistrar, KernelReaderRegistrar,
    MailboxCache, OutboxRouter, ProfileLookup, RelayConnectedHookRegistrar,
    RelayTextInterceptorRegistrar, ReqFrameInterceptorRegistrar, RoutingFactoryRegistrar,
    RoutingTraceObserver,
};
use nmp_core::KernelReducer;
use nmp_coverage_gate::CoverageGate;
use nmp_router::{
    GenericOutboxRouter, InMemoryBlockedRelayCache, InMemoryMailboxCache, IndexerRepublishPolicy,
    Kind10006Parser, Nip65OutboxResolver,
};
use nmp_store::EventStore;

pub mod ownership;

/// Substrate-only configuration.
///
/// The same coverage gate value feeds both the coverage trimming hook and the
/// NIP-77 interceptors. App roots that override this policy pass one value here;
/// there is no separate defaults config object.
#[derive(Clone, Debug)]
pub struct SubstrateConfig {
    /// D2 coverage policy shared by the coverage hook and NIP-77 runtime.
    pub coverage_gate: CoverageGate,
}

impl Default for SubstrateConfig {
    fn default() -> Self {
        Self {
            coverage_gate: CoverageGate::default(),
        }
    }
}

/// Handles returned by [`install`].
#[derive(Clone)]
pub struct SubstrateHandles {
    /// Shared NIP-65 mailbox cache read handle.
    ///
    /// This is the same instance installed as the NIP-19 encoder reader, the
    /// routing factory cache, and the kind:10002 parser writer.
    pub mailbox_cache: Arc<dyn MailboxCache>,
}

/// Shared substrate read-model wiring.
#[derive(Clone)]
pub struct SubstrateWiring {
    mailbox_cache: Arc<InMemoryMailboxCache>,
    profile_cache: Arc<nmp_nip01::ProfileCache>,
    contacts_cache: Arc<nmp_nip01::ContactsCache>,
}

impl Default for SubstrateWiring {
    fn default() -> Self {
        Self::new()
    }
}

impl SubstrateWiring {
    /// Construct fresh process-lifetime caches for one composition root.
    #[must_use]
    pub fn new() -> Self {
        Self {
            mailbox_cache: Arc::new(InMemoryMailboxCache::new()),
            profile_cache: Arc::new(nmp_nip01::ProfileCache::new()),
            contacts_cache: Arc::new(nmp_nip01::ContactsCache::new()),
        }
    }

    /// The NMP-owned NIP-65 (kind:10002) mailbox cache as a read handle (#2085).
    ///
    /// Returns a clone of the **same** `Arc<InMemoryMailboxCache>` this wiring
    /// hands to all three substrate seams: the NIP-19 encoder's
    /// `set_mailbox_cache_reader`, the routing-substrate factory, and the
    /// `nmp_router::Kind10002Parser` writer. Instance identity is load-bearing —
    /// app-core crates that read a pubkey's importable relay list (e.g.
    /// Highlighter's relay-import preview) MUST read through this handle rather
    /// than constructing a fresh `InMemoryMailboxCache`, which would be empty
    /// and divergent from the cache the parser actually writes.
    ///
    /// The handle preserves the read/write/both role shape via
    /// [`MailboxCache::snapshot`]; it does not expose raw event history.
    #[must_use]
    pub fn mailbox_cache(&self) -> Arc<dyn MailboxCache> {
        self.mailbox_cache.clone()
    }

    /// Install the shared cache/parser pairs on an AppHost-style composition
    /// target.
    ///
    /// This intentionally covers only the cache/parser/router construction.
    /// Full app/runtime roots should call [`install`] so the publish resolver,
    /// coverage, NIP-77, blocked-relay, and NIP-11 substrate are installed too.
    ///
    /// Returns the shared NIP-65 mailbox cache read handle (#2085) — the same
    /// `Arc` installed as the encoder reader, the routing factory cache, and the
    /// kind:10002 parser writer (see [`Self::mailbox_cache`]).
    pub fn install_on_app_host(
        &self,
        app: &(impl IngestParserRegistrar + KernelReaderRegistrar + RoutingFactoryRegistrar),
    ) -> Arc<dyn MailboxCache> {
        let mailbox_reader: Arc<dyn MailboxCache> = self.mailbox_cache.clone();
        app.set_mailbox_cache_reader(Arc::clone(&mailbox_reader));

        let cache_for_factory = Arc::clone(&self.mailbox_cache);
        app.set_routing_substrate(
            move |observer: Arc<dyn RoutingTraceObserver>|
                  -> (Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>) {
                let router: Arc<dyn OutboxRouter> =
                    Arc::new(GenericOutboxRouter::new().with_trace_observer(observer));
                let cache: Arc<dyn MailboxCache> = cache_for_factory.clone();
                (router, cache)
            },
        );

        self.install_reader_parser_pairs(app);
        mailbox_reader
    }

    /// Install the shared cache/parser pairs on a reducer-owned web
    /// composition root.
    pub fn install_on_reducer(&self, reducer: &mut KernelReducer) {
        let router: Arc<dyn OutboxRouter> = Arc::new(GenericOutboxRouter::new());
        let mailbox_reader: Arc<dyn MailboxCache> = self.mailbox_cache.clone();
        reducer.set_routing(router, mailbox_reader);

        let kind10002_parser: Arc<dyn IngestParser> = Arc::new(nmp_router::Kind10002Parser::new(
            Arc::clone(&self.mailbox_cache),
        ));
        reducer.register_ingest_parser(10_002, kind10002_parser);

        self.install_profile_contacts_on_reducer(reducer);
    }

    fn install_reader_parser_pairs(
        &self,
        app: &(impl IngestParserRegistrar + KernelReaderRegistrar),
    ) {
        let kind10002_parser: Arc<dyn IngestParser> = Arc::new(nmp_router::Kind10002Parser::new(
            Arc::clone(&self.mailbox_cache),
        ));
        app.register_ingest_parser(10_002, kind10002_parser);

        let profile_lookup: Arc<dyn ProfileLookup> = self.profile_cache.clone();
        app.set_profile_lookup(profile_lookup);
        let kind0_parser: Arc<dyn IngestParser> =
            Arc::new(nmp_nip01::Kind0Parser::new(Arc::clone(&self.profile_cache)));
        app.register_ingest_parser(0, kind0_parser);

        let contacts_lookup: Arc<dyn ContactsLookup> = self.contacts_cache.clone();
        app.set_contacts_lookup(contacts_lookup);
        let kind3_parser: Arc<dyn IngestParser> = Arc::new(nmp_nip01::Kind3Parser::new(
            Arc::clone(&self.contacts_cache),
        ));
        app.register_ingest_parser(3, kind3_parser);
    }

    fn install_profile_contacts_on_reducer(&self, reducer: &mut KernelReducer) {
        let profile_lookup: Arc<dyn ProfileLookup> = self.profile_cache.clone();
        reducer.set_profile_lookup(profile_lookup);
        let kind0_parser: Arc<dyn IngestParser> =
            Arc::new(nmp_nip01::Kind0Parser::new(Arc::clone(&self.profile_cache)));
        reducer.register_ingest_parser(0, kind0_parser);

        let contacts_lookup: Arc<dyn ContactsLookup> = self.contacts_cache.clone();
        reducer.set_contacts_lookup(contacts_lookup);
        let kind3_parser: Arc<dyn IngestParser> = Arc::new(nmp_nip01::Kind3Parser::new(
            Arc::clone(&self.contacts_cache),
        ));
        reducer.register_ingest_parser(3, kind3_parser);
    }
}

/// Install fresh substrate cache/parser/router wiring on an AppHost-style
/// composition target.
///
/// Returns the shared NIP-65 mailbox cache read handle (#2085) — the same `Arc`
/// installed as the encoder reader, the routing factory cache, and the
/// kind:10002 parser writer.
pub fn install_on_app_host(
    app: &(impl IngestParserRegistrar + KernelReaderRegistrar + RoutingFactoryRegistrar),
) -> Arc<dyn MailboxCache> {
    SubstrateWiring::new().install_on_app_host(app)
}

/// Install fresh substrate cache/parser/router wiring on a reducer-owned web
/// composition root.
pub fn install_on_reducer(reducer: &mut KernelReducer) {
    SubstrateWiring::new().install_on_reducer(reducer);
}

/// Install the full substrate floor every NMP app/runtime root needs.
///
/// This is substrate correctness only: routing action, shared cache/parser
/// wiring, blocked-relay parser/actions, publish resolver, raw-event forwarding,
/// coverage trimming, NIP-77 interceptors, and native NIP-11 relay metadata.
/// Product/protocol features such as follows, DMs, search scopes, mutes,
/// comments, longform, WOT, and app relays are composed separately by their
/// owner crates or app root.
pub fn install(
    app: &mut (impl ActionRegistrar
              + BlockedRelayLookupRegistrar
              + CoverageHookRegistrar
              + IngestParserRegistrar
              + KernelReaderRegistrar
              + RelayConnectedHookRegistrar
              + RelayTextInterceptorRegistrar
              + ReqFrameInterceptorRegistrar
              + RoutingFactoryRegistrar),
    config: SubstrateConfig,
) -> SubstrateHandles {
    nmp_router::register_actions(app);

    let mailbox_cache = install_on_app_host(app);

    let blocked_cache: Arc<InMemoryBlockedRelayCache> = Arc::new(InMemoryBlockedRelayCache::new());
    app.set_blocked_relay_lookup(
        Arc::clone(&blocked_cache) as Arc<dyn nmp_core::substrate::BlockedRelayLookup>
    );
    let blocked_parser: Arc<dyn nmp_core::substrate::IngestParser> =
        Arc::new(Kind10006Parser::new(Arc::clone(&blocked_cache)));
    app.register_ingest_parser(10_006, blocked_parser);
    nmp_router::register_block_relay_actions(app, blocked_cache);

    app.set_publish_resolver_factory(
        |store: Arc<dyn EventStore>,
         indexer_relays: IndexerRelaysSlot,
         local_write_relays: LocalWriteRelaysSlot,
         active_account: ActiveAccountSlot|
         -> Arc<dyn OutboxResolver> {
            Arc::new(Nip65OutboxResolver::with_local_relays(
                store,
                indexer_relays,
                local_write_relays,
                active_account,
            ))
        },
    );

    app.set_external_event_sink_policy_factory(|context| {
        vec![Arc::new(IndexerRepublishPolicy::enabled(context)) as Arc<dyn ExternalEventSinkPolicy>]
    });

    let gate = config.coverage_gate;
    let negentropy_runtime = Arc::new(nmp_nip77::NegentropySyncRuntime::new(gate.clone()));
    let req_interceptor: Arc<dyn nmp_core::substrate::ReqFrameInterceptor> =
        negentropy_runtime.clone();
    let relay_interceptor: Arc<dyn nmp_core::substrate::RelayTextInterceptor> = negentropy_runtime;
    app.set_req_frame_interceptor(req_interceptor);
    app.add_relay_text_interceptor(relay_interceptor);
    app.set_coverage_hook(Arc::new(move |plan| {
        let cap = gate.max_relay_connections;
        if plan.per_relay.len() > cap {
            let keep: Vec<_> = plan.per_relay.keys().take(cap).cloned().collect();
            plan.per_relay.retain(|k, _| keep.contains(k));
        }
    }));

    #[cfg(feature = "native")]
    nmp_nip11::register(app);

    SubstrateHandles { mailbox_cache }
}
