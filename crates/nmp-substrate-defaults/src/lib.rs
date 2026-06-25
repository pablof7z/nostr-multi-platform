//! Wasm-safe default substrate cache/parser wiring.
//!
//! This crate owns the shared construction that ADR-0046 calls "un-copyable":
//! one mailbox cache shared by the NIP-65 reader, router factory, and
//! kind:10002 parser; one profile cache shared by the kernel profile reader and
//! kind:0 parser; and one contacts cache shared by the kernel contacts reader
//! and kind:3 parser.
//!
//! # Shared composition target (issues #2059/#2060)
//!
//! This is an **internal helper crate** — both the native and browser builders
//! wiring happens through a single door: `nmp_defaults::register_defaults`.
//! The browser builder MUST NOT call this crate's functions directly (the D4
//! doctrine gate #2082 will enforce it). Instead:
//!
//! 1. Browser builder implements the narrow `AppHost` registrar traits
//! 2. Browser builder calls `nmp_defaults::register_defaults(app: &mut impl AppHost)`
//! 3. `register_defaults` internally calls `nmp_substrate_defaults::install_on_app_host`
//!    for both native and browser (same code path, substrate-agnostic)
//! 4. Browser builder receives the canonical floor: routing/mailbox/profile/contact
//!    caches + kind:10002/0/3 parsers + publish-resolver + coverage hooks, all
//!    installed exactly once with zero hand-copied defaults.
//!
//! The reducer-owned web (`KernelReducer` browser harness) calls the reducer
//! installer `install_on_reducer` to populate the reducer without an AppHost.
//!
//! `nmp-defaults::register_substrate` calls this crate for the cache/parser pairs,
//! then installs the native-specific collaborators (publish resolver, raw forwarding,
//! coverage, NIP-11). This separation preserves the single-sourced un-copyable block
//! while keeping native-only I/O out of the wasm-safe path.

use std::sync::Arc;

use nmp_core::substrate::{
    ContactsLookup, IngestParser, IngestParserRegistrar, KernelReaderRegistrar, MailboxCache,
    OutboxRouter, ProfileLookup, RoutingFactoryRegistrar, RoutingTraceObserver,
};
use nmp_core::KernelReducer;
use nmp_router::{GenericOutboxRouter, InMemoryMailboxCache};

/// Shared default substrate read-model wiring.
#[derive(Clone)]
pub struct DefaultSubstrateWiring {
    mailbox_cache: Arc<InMemoryMailboxCache>,
    profile_cache: Arc<nmp_nip01::ProfileCache>,
    contacts_cache: Arc<nmp_nip01::ContactsCache>,
}

impl Default for DefaultSubstrateWiring {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultSubstrateWiring {
    /// Construct fresh process-lifetime caches for one composition root.
    #[must_use]
    pub fn new() -> Self {
        Self {
            mailbox_cache: Arc::new(InMemoryMailboxCache::new()),
            profile_cache: Arc::new(nmp_nip01::ProfileCache::new()),
            contacts_cache: Arc::new(nmp_nip01::ContactsCache::new()),
        }
    }

    /// Install the shared cache/parser pairs on an AppHost-style composition
    /// target.
    ///
    /// This intentionally covers only the wasm-safe cache/parser/router
    /// construction. The full native substrate tier still belongs to
    /// `nmp-defaults::register_substrate`.
    pub fn install_on_app_host(
        &self,
        app: &(impl IngestParserRegistrar + KernelReaderRegistrar + RoutingFactoryRegistrar),
    ) {
        let mailbox_reader: Arc<dyn MailboxCache> = self.mailbox_cache.clone();
        app.set_mailbox_cache_reader(mailbox_reader);

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

/// Install fresh default substrate wiring on an AppHost-style composition
/// target.
pub fn install_on_app_host(
    app: &(impl IngestParserRegistrar + KernelReaderRegistrar + RoutingFactoryRegistrar),
) {
    DefaultSubstrateWiring::new().install_on_app_host(app);
}

/// Install fresh default substrate wiring on a reducer-owned web composition
/// root.
pub fn install_on_reducer(reducer: &mut KernelReducer) {
    DefaultSubstrateWiring::new().install_on_reducer(reducer);
}
