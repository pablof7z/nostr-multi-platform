use std::ops::Range;
use std::sync::{Arc, Mutex};

use nmp_core::publish::OutboxResolver;
use nmp_core::slots::{
    new_indexer_relays_slot, ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use nmp_core::subs::PlanCoverageHook;
use nmp_core::substrate::{
    ActionModule, ActionRegistrar, BlockedRelayLookup, BlockedRelayLookupRegistrar,
    CoverageHookRegistrar, ExternalEventSinkPolicy, IngestOutcomeKind, IngestParser,
    IngestParserRegistrar, KernelReaderRegistrar, MailboxCache, OutboxRouter, ProfileLookup,
    RawEventForwardPolicyContext, RegistrationError, RelayConnectedHook,
    RelayConnectedHookRegistrar, RelayTextInterceptor, RelayTextInterceptorRegistrar,
    ReqFrameInterceptor, ReqFrameInterceptorRegistrar, RoutingFactoryRegistrar,
    RoutingTraceObserver, SignedEventFrame,
};
use nmp_store::{EventStore, MemEventStore, RawEvent};

use super::{install, SubstrateConfig};

type ExternalEventSinkPolicyFactory =
    dyn Fn(RawEventForwardPolicyContext) -> Vec<Arc<dyn ExternalEventSinkPolicy>> + Send + Sync;

#[derive(Default)]
struct CapturingHost {
    external_event_sink_policy_factory: Mutex<Option<Arc<ExternalEventSinkPolicyFactory>>>,
    mailbox_cache_reader: Mutex<Option<Arc<dyn MailboxCache>>>,
}

impl ActionRegistrar for CapturingHost {
    fn register_action<M: ActionModule + 'static>(
        &mut self,
        _module: M,
    ) -> Result<(), RegistrationError> {
        Ok(())
    }
}

impl IngestParserRegistrar for CapturingHost {
    fn register_ingest_parser(&self, _kind: u32, _parser: Arc<dyn IngestParser>) {}

    fn replace_ingest_parser(
        &self,
        _kind: u32,
        _slot_key: &'static str,
        _parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        None
    }

    fn unregister_ingest_parser(&self, _kind: u32, _slot_key: &'static str) {}

    fn replace_ingest_parser_range(
        &self,
        _range: Range<u32>,
        _slot_key: &'static str,
        _parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        None
    }

    fn unregister_ingest_parser_range(&self, _slot_key: &'static str) {}
}

impl KernelReaderRegistrar for CapturingHost {
    fn set_profile_lookup(&self, _lookup: Arc<dyn ProfileLookup>) {}

    fn set_mailbox_cache_reader(&self, cache: Arc<dyn MailboxCache>) {
        *self
            .mailbox_cache_reader
            .lock()
            .expect("mailbox cache reader slot") = Some(cache);
    }
}

impl BlockedRelayLookupRegistrar for CapturingHost {
    fn set_blocked_relay_lookup(&self, _lookup: Arc<dyn BlockedRelayLookup>) {}
}

impl RoutingFactoryRegistrar for CapturingHost {
    fn set_routing_substrate<F>(&self, _factory: F)
    where
        F: Fn(Arc<dyn RoutingTraceObserver>) -> (Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)
            + Send
            + Sync
            + 'static,
    {
    }

    fn set_publish_resolver_factory<F>(&self, _factory: F)
    where
        F: Fn(
                Arc<dyn EventStore>,
                Arc<dyn MailboxCache>,
                IndexerRelaysSlot,
                LocalWriteRelaysSlot,
                ActiveAccountSlot,
            ) -> Arc<dyn OutboxResolver>
            + Send
            + Sync
            + 'static,
    {
    }

    fn set_external_event_sink_policy_factory<F>(&self, factory: F)
    where
        F: Fn(RawEventForwardPolicyContext) -> Vec<Arc<dyn ExternalEventSinkPolicy>>
            + Send
            + Sync
            + 'static,
    {
        let factory: Arc<ExternalEventSinkPolicyFactory> = Arc::new(factory);
        *self
            .external_event_sink_policy_factory
            .lock()
            .expect("external policy factory slot") = Some(factory);
    }

    fn set_nostrconnect_bootstrap_relay(&self, _url: String) {}

    fn set_nostrconnect_perms(&self, _perms: String) {}

    fn set_relay_user_agent(&self, _user_agent: String) {}

    fn set_outbound_public_tags(&self, _tags: Vec<Vec<String>>) {}
}

impl CoverageHookRegistrar for CapturingHost {
    fn set_coverage_hook(&self, _hook: PlanCoverageHook) {}
}

impl ReqFrameInterceptorRegistrar for CapturingHost {
    fn set_req_frame_interceptor(&self, _interceptor: Arc<dyn ReqFrameInterceptor>) {}
}

impl RelayTextInterceptorRegistrar for CapturingHost {
    fn add_relay_text_interceptor(&self, _interceptor: Arc<dyn RelayTextInterceptor>) {}
}

impl RelayConnectedHookRegistrar for CapturingHost {
    fn add_relay_connected_hook(&self, _hook: Arc<dyn RelayConnectedHook>) {}
}

fn context_with_indexers(urls: &[&str]) -> RawEventForwardPolicyContext {
    let slot = new_indexer_relays_slot();
    {
        let mut guard = slot.lock().expect("indexer relays slot");
        guard.replace(urls.iter().map(|url| (*url).to_string()).collect());
    }
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    RawEventForwardPolicyContext::new(store, slot)
}

fn frame(id_byte: u8) -> SignedEventFrame {
    let raw = RawEvent {
        id: format!("{:02x}{}", id_byte, "00".repeat(31)),
        pubkey: "11".repeat(32),
        created_at: 1_700_000_000,
        kind: 0,
        tags: Vec::new(),
        content: String::new(),
        sig: "22".repeat(64),
    };
    SignedEventFrame::build(
        Arc::new(raw),
        Some(Arc::<str>::from("wss://content-relay/")),
        IngestOutcomeKind::Inserted,
    )
    .expect("signed event frame")
}

#[test]
fn install_returns_live_indexer_republish_handle() {
    let mut host = CapturingHost::default();

    let handles = install(&mut host, SubstrateConfig::default());

    let factory = host
        .external_event_sink_policy_factory
        .lock()
        .expect("external policy factory slot")
        .clone()
        .expect("install registers external event sink policy factory");
    let policies = factory(context_with_indexers(&["wss://indexer/"]));
    assert_eq!(policies.len(), 1);

    assert!(handles.indexer_republish.is_enabled());
    assert_eq!(handles.indexer_republish.forwarded_count(), 0);
    assert_eq!(
        policies[0].destinations(&frame(0x31)).len(),
        1,
        "initially enabled substrate policy forwards"
    );
    assert_eq!(
        handles.indexer_republish.forwarded_count(),
        1,
        "returned substrate handle observes installed policy forwards"
    );

    handles.indexer_republish.set_enabled(false);
    assert!(
        policies[0].destinations(&frame(0x32)).is_empty(),
        "returned substrate handle disables the installed policy"
    );
    assert_eq!(
        handles.indexer_republish.forwarded_count(),
        1,
        "disabled substrate policy must not increment forwarded count"
    );

    handles.indexer_republish.set_enabled(true);
    assert_eq!(
        policies[0].destinations(&frame(0x33)).len(),
        1,
        "returned substrate handle re-enables the installed policy"
    );
    assert_eq!(handles.indexer_republish.forwarded_count(), 2);
}

#[test]
fn install_returns_same_mailbox_cache_reader_it_installs() {
    let mut host = CapturingHost::default();

    let handles = install(&mut host, SubstrateConfig::default());
    let installed = host
        .mailbox_cache_reader
        .lock()
        .expect("mailbox cache reader slot")
        .clone()
        .expect("install registers mailbox cache reader");

    assert!(
        Arc::ptr_eq(&handles.mailbox_cache, &installed),
        "install must return the same read-only mailbox cache handle it installs"
    );
}
