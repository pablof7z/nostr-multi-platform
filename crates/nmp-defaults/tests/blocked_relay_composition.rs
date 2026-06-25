//! Composition proof: `register_substrate` wires `Kind10006Parser` and the
//! `BlockedRelayLookup` onto the SAME `InMemoryBlockedRelayCache` so ingesting
//! a kind:10006 event via the parser populates the lookup the kernel reads.
//!
//! The canonical composition pattern (one Arc, two roles) is shared with the
//! kind:10002 mailbox-cache block in `register_substrate`. This test proves
//! the analogous wiring for kind:10006 blocked relays is correct.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use nmp_core::publish::OutboxResolver;
use nmp_core::slots::{ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot};
use nmp_store::{EventStore, RawEvent, VerifiedEvent};
use nmp_core::subs::PlanCoverageHook;
use nmp_core::substrate::{
    ActionModule, ActionRegistrar, BlockedRelayLookup, BlockedRelayLookupRegistrar,
    CoverageHookRegistrar, IngestParser, IngestParserRegistrar, KernelReaderRegistrar,
    MailboxCache, OutboxRouter,
    RelayConnectedHook, RelayConnectedHookRegistrar, RelayTextInterceptor,
    RelayTextInterceptorRegistrar, ReqFrameInterceptor, ReqFrameInterceptorRegistrar,
    RoutingFactoryRegistrar, RoutingTraceObserver,
};
use nmp_coverage_gate::CoverageGate;

// ─── CompositionSpy ──────────────────────────────────────────────────────────

/// Captures the blocked-relay lookup and every ingest parser registered by
/// `register_substrate`. Implements exactly the traits the function requires.
#[derive(Default)]
struct CompositionSpy {
    lookup: Mutex<Option<Arc<dyn BlockedRelayLookup>>>,
    parsers: Mutex<Vec<(u32, Arc<dyn IngestParser>)>>,
}

impl ActionRegistrar for CompositionSpy {
    fn register_action<M: ActionModule + 'static>(
        &mut self,
        _module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        Ok(())
    }
}

impl BlockedRelayLookupRegistrar for CompositionSpy {
    fn set_blocked_relay_lookup(&self, lookup: Arc<dyn BlockedRelayLookup>) {
        *self.lookup.lock().unwrap() = Some(lookup);
    }
}

impl CoverageHookRegistrar for CompositionSpy {
    fn set_coverage_hook(&self, _hook: PlanCoverageHook) {}
}

impl IngestParserRegistrar for CompositionSpy {
    fn register_ingest_parser(&self, kind: u32, parser: Arc<dyn IngestParser>) {
        self.parsers.lock().unwrap().push((kind, parser));
    }

    fn replace_ingest_parser(
        &self,
        _kind: u32,
        _slot_key: &'static str,
        _parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        unreachable!("register_substrate does not replace ingest parsers");
    }

    fn unregister_ingest_parser(&self, _kind: u32, _slot_key: &'static str) {
        unreachable!("register_substrate does not unregister ingest parsers");
    }

    fn replace_ingest_parser_range(
        &self,
        _range: Range<u32>,
        _slot_key: &'static str,
        _parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        unreachable!("register_substrate does not replace ingest-parser ranges");
    }

    fn unregister_ingest_parser_range(&self, _slot_key: &'static str) {
        unreachable!("register_substrate does not unregister ingest-parser ranges");
    }
}

impl KernelReaderRegistrar for CompositionSpy {
    fn set_profile_lookup(&self, _lookup: Arc<dyn nmp_core::substrate::ProfileLookup>) {}
    fn set_contacts_lookup(&self, _lookup: Arc<dyn nmp_core::substrate::ContactsLookup>) {}
    fn set_mailbox_cache_reader(&self, _cache: Arc<dyn MailboxCache>) {}
}

impl RelayConnectedHookRegistrar for CompositionSpy {
    fn add_relay_connected_hook(&self, _hook: Arc<dyn RelayConnectedHook>) {}
}

impl RelayTextInterceptorRegistrar for CompositionSpy {
    fn add_relay_text_interceptor(&self, _interceptor: Arc<dyn RelayTextInterceptor>) {}
}

impl ReqFrameInterceptorRegistrar for CompositionSpy {
    fn set_req_frame_interceptor(&self, _interceptor: Arc<dyn ReqFrameInterceptor>) {}
}

impl RoutingFactoryRegistrar for CompositionSpy {
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
                IndexerRelaysSlot,
                LocalWriteRelaysSlot,
                ActiveAccountSlot,
            ) -> Arc<dyn OutboxResolver>
            + Send
            + Sync
            + 'static,
    {
    }

    fn set_nostrconnect_bootstrap_relay(&self, _url: String) {
        unreachable!("register_substrate does not set the nostrconnect bootstrap relay");
    }

    fn set_nostrconnect_perms(&self, _perms: String) {
        unreachable!("register_substrate does not set the nostrconnect perms");
    }

    fn set_relay_user_agent(&self, _user_agent: String) {
        // No-op in test; User-Agent is wired by register_defaults_with.
    }

    fn set_outbound_public_tags(&self, _tags: Vec<Vec<String>>) {
        // No-op in test; outbound tags are wired by register_defaults_with.
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn kind10006_event(pubkey: &str, relay_urls: &[&str]) -> VerifiedEvent {
    let tags: Vec<Vec<String>> = relay_urls
        .iter()
        .map(|url| vec!["relay".to_string(), url.to_string()])
        .collect();
    VerifiedEvent::from_raw_unchecked(RawEvent {
        id: "00".repeat(32),
        pubkey: pubkey.to_string(),
        created_at: 0,
        kind: 10_006,
        tags,
        content: String::new(),
        sig: "22".repeat(64),
    })
}

// ─── Test ─────────────────────────────────────────────────────────────────────

#[test]
fn register_substrate_wires_kind10006_parser_and_lookup_on_shared_arc() {
    // Arrange: run register_substrate against our spy.
    let mut spy = CompositionSpy::default();
    nmp_defaults::register_substrate(&mut spy, CoverageGate::default());

    // Verify the lookup was installed.
    let lookup = spy
        .lookup
        .lock()
        .unwrap()
        .clone()
        .expect("register_substrate must call set_blocked_relay_lookup");

    // Verify the kind:10006 parser was registered.
    let parsers = spy.parsers.lock().unwrap().clone();
    let (_, kind10006_parser) = parsers
        .iter()
        .find(|(kind, _)| *kind == 10_006)
        .expect("register_substrate must register a kind:10006 ingest parser");

    // Act: feed a kind:10006 event through the parser.
    let evt = kind10006_event("alice", &["wss://blocked.example"]);
    kind10006_parser.parse(&evt);

    // Assert: the lookup (sharing the same Arc) now returns the blocked relay.
    let blocked = lookup.blocked_relays("alice");
    assert!(
        blocked.contains(&"wss://blocked.example".to_string()),
        "snapshot_blocked_relays must read from the same InMemoryBlockedRelayCache \
         the Kind10006Parser writes into — the two must share one Arc"
    );
}
