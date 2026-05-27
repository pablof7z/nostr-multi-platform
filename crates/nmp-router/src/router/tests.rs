use super::*;
use std::sync::Arc;

use nmp_core::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape,
};
use nmp_core::substrate::{
    BlockedRelaySet, MailboxCache, ParsedRelayList, SessionKeySet,
};

use crate::InMemoryMailboxCache;

fn pubkey() -> String {
    "alice".into()
}

fn unsigned() -> UnsignedEvent {
    UnsignedEvent {
        pubkey: pubkey(),
        kind: 1,
        tags: vec![],
        content: String::new(),
        created_at: 0,
    }
}

fn interest_for(authors: &[&str]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(0),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|s| (*s).into()).collect(),
            ..InterestShape::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

fn ctx<'a>(
    cache: &'a dyn MailboxCache,
    blocked: &'a BlockedRelaySet,
    explicit: Option<&'a [String]>,
    app_relays: &'a [String],
) -> RoutingContext<'a> {
    RoutingContext {
        active_account: None,
        session_keys: SessionKeySet {
            app_relays,
            ..SessionKeySet::default()
        },
        mailbox_cache: cache,
        blocked_relays: blocked,
        explicit_targets: explicit,
    }
}

fn ctx_with_indexers<'a>(
    cache: &'a dyn MailboxCache,
    blocked: &'a BlockedRelaySet,
    app_relays: &'a [String],
    indexer_relays: &'a [String],
) -> RoutingContext<'a> {
    RoutingContext {
        active_account: None,
        session_keys: SessionKeySet {
            app_relays,
            indexer_relays,
            ..SessionKeySet::default()
        },
        mailbox_cache: cache,
        blocked_relays: blocked,
        explicit_targets: None,
    }
}

fn interest_for_kinds(authors: &[&str], kinds: &[u32]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(0),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|s| (*s).into()).collect(),
            kinds: kinds.iter().copied().collect(),
            ..InterestShape::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

#[test]
fn publish_explicit_targets_skips_generic_algorithm() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let explicit = vec!["wss://forced.example".to_string()];
    let app = vec!["wss://app.example".to_string()];
    let c = ctx(&cache, &blocked, Some(&explicit), &app);

    let router = GenericOutboxRouter::new();
    let r = router.route_publish(&unsigned(), &c).unwrap();
    let urls: Vec<&String> = r.urls().collect();

    assert_eq!(urls, vec![&"wss://forced.example".to_string()]);
    // AppRelay was configured but explicit_targets shortcut it.
    for sources in r.relays.values() {
        for s in sources {
            assert!(matches!(s, RoutingSource::ClassRouted { .. }));
        }
    }
}

#[test]
fn publish_uses_nip65_write_set() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert(pubkey(), ParsedRelayList {
        write: vec!["wss://w.example".into()],
        both: vec!["wss://b.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&*cache, &blocked, None, &app);

    let router = GenericOutboxRouter::new();
    let r = router.route_publish(&unsigned(), &c).unwrap();

    let urls: std::collections::HashSet<&String> = r.urls().collect();
    let w = "wss://w.example".to_string();
    let b = "wss://b.example".to_string();
    assert!(urls.contains(&w));
    assert!(urls.contains(&b));
    for sources in r.relays.values() {
        assert!(sources.iter().any(|s| matches!(
            s,
            RoutingSource::Nip65 { direction: Direction::Write }
        )));
    }
}

#[test]
fn publish_app_relay_fallback_when_no_nip65() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let c = ctx(&cache, &blocked, None, &app);

    let router = GenericOutboxRouter::new();
    let r = router.route_publish(&unsigned(), &c).unwrap();
    let urls: Vec<&String> = r.urls().collect();
    assert_eq!(urls, vec![&"wss://app.example".to_string()]);
    for sources in r.relays.values() {
        assert!(sources.iter().any(|s| matches!(
            s,
            RoutingSource::AppRelay { mode: AppRelayMode::Fallback }
        )));
    }
}

#[test]
fn publish_unroutable_when_no_nip65_and_no_app_relay() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&cache, &blocked, None, &app);

    let router = GenericOutboxRouter::new();
    let err = router.route_publish(&unsigned(), &c).unwrap_err();
    assert_eq!(err, RoutingError::Unroutable(pubkey()));
}

#[test]
fn publish_drops_blocked_relays() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert(pubkey(), ParsedRelayList {
        write: vec!["wss://blocked.example".into(), "wss://ok.example".into()],
        ..ParsedRelayList::default()
    });
    let mut blocked = BlockedRelaySet::new();
    blocked.insert("wss://blocked.example".into());
    let app: Vec<String> = vec![];
    let c = ctx(&*cache, &blocked, None, &app);

    let router = GenericOutboxRouter::new();
    let r = router.route_publish(&unsigned(), &c).unwrap();
    let urls: Vec<&String> = r.urls().collect();
    assert_eq!(urls, vec![&"wss://ok.example".to_string()]);
}

#[test]
fn subscribe_uses_nip65_read_set_for_each_author() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert("alice".into(), ParsedRelayList {
        read: vec!["wss://alice-r.example".into()],
        ..ParsedRelayList::default()
    });
    cache.upsert("bob".into(), ParsedRelayList {
        both: vec!["wss://bob-b.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&*cache, &blocked, None, &app);

    let router = GenericOutboxRouter::new();
    let r = router
        .route_subscription(&interest_for(&["alice", "bob"]), &c)
        .unwrap();
    let urls: std::collections::HashSet<&String> = r.urls().collect();
    assert!(urls.contains(&"wss://alice-r.example".to_string()));
    assert!(urls.contains(&"wss://bob-b.example".to_string()));
}

#[test]
fn subscribe_explicit_targets_shortcuts() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert("alice".into(), ParsedRelayList {
        read: vec!["wss://from-cache.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let explicit = vec!["wss://override.example".to_string()];
    let app: Vec<String> = vec![];
    let c = ctx(&*cache, &blocked, Some(&explicit), &app);

    let router = GenericOutboxRouter::new();
    let r = router
        .route_subscription(&interest_for(&["alice"]), &c)
        .unwrap();
    let urls: Vec<&String> = r.urls().collect();
    assert_eq!(urls, vec![&"wss://override.example".to_string()]);
}

#[test]
fn subscribe_unroutable_when_no_lane_resolves() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&cache, &blocked, None, &app);

    let router = GenericOutboxRouter::new();
    let err = router
        .route_subscription(&interest_for(&["alice"]), &c)
        .unwrap_err();
    assert_eq!(err, RoutingError::Unroutable("alice".into()));
}

#[test]
fn router_satisfies_dyn_outbox_router_bound() {
    // Compile-test: confirm the impl is object-safe and the kernel can
    // hold it as Arc<dyn OutboxRouter>.
    let _: Box<dyn OutboxRouter> = Box::new(GenericOutboxRouter::new());
}

#[derive(Default)]
struct TestObserver {
    publishes: std::sync::Mutex<Vec<(PublishTrace, usize)>>,
    subscriptions: std::sync::Mutex<Vec<SubscriptionTrace>>,
}

impl RoutingTraceObserver for TestObserver {
    fn on_publish(&self, summary: PublishTrace, routed: &RoutedRelaySet) {
        self.publishes
            .lock()
            .unwrap()
            .push((summary, routed.relays.len()));
    }
    fn on_subscription(&self, summary: SubscriptionTrace, _routed: &RoutedRelaySet) {
        self.subscriptions.lock().unwrap().push(summary);
    }
}

#[test]
fn trace_observer_fires_on_success_and_skips_unroutable() {
    // Two route_publish calls and one route_subscription against a single
    // router+observer instance. Asserts the observer fires once per
    // successful call (with the right trace payload), and NOT at all when
    // the router returns Unroutable.
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert(pubkey(), ParsedRelayList {
        write: vec!["wss://w.example".into()],
        read: vec!["wss://r.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let obs = Arc::new(TestObserver::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);

    // 1. Successful publish — explicit_targets unset.
    let c = ctx(&*cache, &blocked, None, &app);
    let _ = router.route_publish(&unsigned(), &c).unwrap();
    // 2. Successful publish — explicit_targets set.
    let explicit = vec!["wss://forced.example".to_string()];
    let c = ctx(&*cache, &blocked, Some(&explicit), &app);
    let _ = router.route_publish(&unsigned(), &c).unwrap();
    // 3. Successful subscription with a non-default interest id.
    let c = ctx(&*cache, &blocked, None, &app);
    let mut interest = interest_for(&["alice"]);
    interest.id = nmp_core::planner::InterestId(42);
    let _ = router.route_subscription(&interest, &c).unwrap();
    // 4. Unroutable publish (no cache, no app-relay) — observer MUST NOT fire.
    let empty_cache = InMemoryMailboxCache::new();
    let c = ctx(&empty_cache, &blocked, None, &app);
    let _ = router
        .route_publish(
            &UnsignedEvent {
                pubkey: "ghost".into(),
                ..unsigned()
            },
            &c,
        )
        .unwrap_err();

    let pubs = obs.publishes.lock().unwrap();
    assert_eq!(pubs.len(), 2, "two publish successes only");
    assert_eq!(pubs[0].0.kind, 1);
    assert_eq!(pubs[0].0.author, pubkey());
    assert!(!pubs[0].0.explicit_targets_set);
    assert!(pubs[1].0.explicit_targets_set);

    let subs = obs.subscriptions.lock().unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].interest_id, 42);
    assert_eq!(subs[0].authors_count, 1);
    assert!(!subs[0].explicit_targets_set);
}

// ─── V-50: lane 6 (Indexer always-on for discovery kinds) ────────────────

#[test]
fn route_subscription_includes_indexer_lane_for_kind_10002_kind_0_kind_3() {
    // For each of kind:10002 / kind:0 / kind:3 (and a kind:10000 NIP-51
    // bookmark for good measure), routing a subscription with an empty
    // cache and a configured indexer URL must:
    //   1. Resolve to the indexer URL,
    //   2. Attribute that URL to RoutingSource::Indexer.
    // Lane 1 (per-author NIP-65) is empty here so the only resolution
    // pathway is lane 6.
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let indexers = vec!["wss://indexer.example".to_string()];

    let router = GenericOutboxRouter::new();
    for kind in [0u32, 3, 10_000, 10_002, 19_999] {
        let interest = interest_for_kinds(&["alice"], &[kind]);
        let c = ctx_with_indexers(&cache, &blocked, &app, &indexers);
        let r = router
            .route_subscription(&interest, &c)
            .unwrap_or_else(|e| panic!("kind {kind} must resolve via indexer lane: {e:?}"));
        let urls: Vec<&String> = r.urls().collect();
        assert_eq!(
            urls,
            vec![&"wss://indexer.example".to_string()],
            "kind {kind} subscription must include the indexer URL"
        );
        let sources = r.relays.get(&"wss://indexer.example".to_string()).unwrap();
        assert!(
            sources.contains(&RoutingSource::Indexer),
            "kind {kind} indexer URL must carry RoutingSource::Indexer attribution; got {sources:?}"
        );
    }
}

#[test]
fn route_subscription_skips_indexer_lane_for_content_kinds() {
    // kind:1 / kind:6 are content kinds — lane 6 must NOT fire. With
    // empty NIP-65 cache and a configured indexer URL the router must
    // surface Unroutable rather than silently routing content to the
    // operator's indexer (which would defeat T105 outbox discipline).
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let indexers = vec!["wss://indexer.example".to_string()];
    let interest = interest_for_kinds(&["alice"], &[1, 6]);
    let c = ctx_with_indexers(&cache, &blocked, &app, &indexers);
    let router = GenericOutboxRouter::new();
    let err = router.route_subscription(&interest, &c).unwrap_err();
    assert_eq!(err, RoutingError::Unroutable("alice".into()));
}

#[test]
fn route_subscription_stacks_indexer_on_top_of_stale_nip65_kind_10002() {
    // V-50 self-seal regression: seed the mailbox cache with
    // { alice -> ["wss://stale.example"] } and issue a kind:10002
    // refresh for alice. The resolved set must include BOTH the cached
    // (stale) URL via Nip65/Read AND the configured indexer URL via
    // RoutingSource::Indexer — so a newer kind:10002 published on a
    // different relay is structurally reachable.
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert("alice".into(), ParsedRelayList {
        read: vec!["wss://stale.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let indexers = vec!["wss://indexer.example".to_string()];
    let interest = interest_for_kinds(&["alice"], &[10_002]);
    let c = ctx_with_indexers(&*cache, &blocked, &app, &indexers);

    let router = GenericOutboxRouter::new();
    let r = router.route_subscription(&interest, &c).unwrap();
    let urls: std::collections::BTreeSet<&String> = r.urls().collect();
    let stale = "wss://stale.example".to_string();
    let indexer = "wss://indexer.example".to_string();
    assert!(
        urls.contains(&stale),
        "lane 1 (stale NIP-65 read) must still resolve — got {urls:?}"
    );
    assert!(
        urls.contains(&indexer),
        "lane 6 (indexer) must STACK on top of lane 1 to defeat the kind:10002 self-seal — got {urls:?}"
    );
    // Lane attribution sanity.
    let stale_sources = r.relays.get(&stale).unwrap();
    assert!(stale_sources.iter().any(|s| matches!(
        s,
        RoutingSource::Nip65 { direction: Direction::Read }
    )));
    let indexer_sources = r.relays.get(&indexer).unwrap();
    assert!(indexer_sources.contains(&RoutingSource::Indexer));
}

#[test]
fn route_publish_includes_indexer_lane_for_discovery_kinds() {
    // R+W symmetric per spec §3.1: publishing a kind:10002 (or kind:0,
    // kind:3, etc.) hits the indexer in addition to the author's
    // NIP-65 write set.
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert(pubkey(), ParsedRelayList {
        write: vec!["wss://w.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let indexers = vec!["wss://indexer.example".to_string()];

    let router = GenericOutboxRouter::new();
    for kind in [0u32, 3, 10_002, 10_000, 19_999] {
        let evt = UnsignedEvent { kind, ..unsigned() };
        let c = ctx_with_indexers(&*cache, &blocked, &app, &indexers);
        let r = router.route_publish(&evt, &c).unwrap();
        let urls: std::collections::BTreeSet<&String> = r.urls().collect();
        let w = "wss://w.example".to_string();
        let i = "wss://indexer.example".to_string();
        assert!(urls.contains(&w), "kind {kind} lane 1 missing");
        assert!(urls.contains(&i), "kind {kind} lane 6 missing");
        assert!(r.relays[&i].contains(&RoutingSource::Indexer));
    }
    // Non-discovery kind:1 — lane 6 must NOT fire.
    let evt = UnsignedEvent { kind: 1, ..unsigned() };
    let c = ctx_with_indexers(&*cache, &blocked, &app, &indexers);
    let r = router.route_publish(&evt, &c).unwrap();
    let urls: std::collections::BTreeSet<&String> = r.urls().collect();
    assert!(urls.contains(&"wss://w.example".to_string()));
    assert!(
        !urls.contains(&"wss://indexer.example".to_string()),
        "kind:1 publish must NOT route to indexer"
    );
}
