use super::*;
use std::sync::Mutex;

/// Trivial cache for substrate-trait tests. Returns whatever was
/// last upserted; never delegates to a real backend.
#[derive(Default)]
struct TestMailboxCache {
    inner: Mutex<BTreeMap<Pubkey, ParsedRelayList>>,
}

impl MailboxCache for TestMailboxCache {
    fn read_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>> {
        self.inner
            .lock()
            .unwrap()
            .get(author)
            .map(ParsedRelayList::read_set)
    }
    fn write_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>> {
        self.inner
            .lock()
            .unwrap()
            .get(author)
            .map(ParsedRelayList::write_set)
    }
    fn snapshot(&self, author: &Pubkey) -> Option<ParsedRelayList> {
        self.inner.lock().unwrap().get(author).cloned()
    }
    fn snapshot_all(&self) -> Vec<(Pubkey, ParsedRelayList)> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    fn remove(&self, author: &Pubkey) {
        self.inner.lock().unwrap().remove(author);
    }
    fn upsert(&self, author: Pubkey, list: ParsedRelayList) {
        self.inner.lock().unwrap().insert(author, list);
    }
}

/// Always-unroutable router for trait shape testing.
struct UnroutableRouter;

impl OutboxRouter for UnroutableRouter {
    fn route_publish(
        &self,
        evt: &UnsignedEvent,
        _ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        Err(RoutingError::Unroutable(evt.pubkey.clone()))
    }
    fn route_subscription(
        &self,
        _interest: &LogicalInterest,
        _ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        Ok(RoutedRelaySet::new())
    }
}

fn ctx_with<'a>(
    cache: &'a dyn MailboxCache,
    blocked: &'a BlockedRelaySet,
    explicit: Option<&'a [RelayUrl]>,
) -> RoutingContext<'a> {
    RoutingContext {
        active_account: None,
        session_keys: SessionKeySet::default(),
        mailbox_cache: cache,
        blocked_relays: blocked,
        explicit_targets: explicit,
    }
}

fn unsigned(pubkey: &str, kind: u32) -> UnsignedEvent {
    UnsignedEvent {
        pubkey: pubkey.into(),
        kind,
        tags: vec![],
        content: String::new(),
        created_at: 0,
    }
}

#[test]
fn parsed_relay_list_read_and_write_sets_include_both() {
    let parsed = ParsedRelayList {
        read: vec!["wss://r.example".into()],
        write: vec!["wss://w.example".into()],
        both: vec!["wss://b.example".into()],
    };
    assert_eq!(
        parsed.read_set(),
        vec!["wss://r.example", "wss://b.example"]
    );
    assert_eq!(
        parsed.write_set(),
        vec!["wss://w.example", "wss://b.example"]
    );
}

#[test]
fn mailbox_cache_known_default_uses_read_or_write_presence() {
    let cache = TestMailboxCache::default();
    let pk: Pubkey = "alice".into();
    assert!(!cache.known(&pk));
    cache.upsert(
        pk.clone(),
        ParsedRelayList {
            read: vec!["wss://r.example".into()],
            ..ParsedRelayList::default()
        },
    );
    assert!(cache.known(&pk));
    assert_eq!(cache.read_relays(&pk), Some(vec!["wss://r.example".into()]),);
}

#[test]
fn routed_relay_set_from_explicit_attributes_class_routed() {
    let urls: Vec<RelayUrl> = vec!["wss://a.example".into(), "wss://b.example".into()];
    let blocked = BlockedRelaySet::new();
    let routed = RoutedRelaySet::from_explicit(&urls, &blocked);

    assert_eq!(routed.urls().count(), 2);
    for sources in routed.relays.values() {
        assert_eq!(sources.len(), 1);
        let s = sources.iter().next().unwrap();
        assert!(matches!(
            s,
            RoutingSource::ClassRouted {
                via: ClassRoutingPath::Explicit,
                ..
            }
        ));
    }
}

#[test]
fn routed_relay_set_from_explicit_drops_blocked() {
    let urls: Vec<RelayUrl> = vec!["wss://a.example".into(), "wss://b.example".into()];
    let mut blocked = BlockedRelaySet::new();
    blocked.insert("wss://a.example".into());

    let routed = RoutedRelaySet::from_explicit(&urls, &blocked);
    let resolved: Vec<&RelayUrl> = routed.urls().collect();
    assert_eq!(resolved, vec![&"wss://b.example".to_string()]);
}

#[test]
fn outbox_router_dyn_dispatch_compiles_and_returns_error() {
    let cache = TestMailboxCache::default();
    let blocked = BlockedRelaySet::new();
    let ctx = ctx_with(&cache, &blocked, None);

    let router: &dyn OutboxRouter = &UnroutableRouter;
    let evt = unsigned("alice", 1);
    let err = router.route_publish(&evt, &ctx).unwrap_err();
    assert_eq!(err, RoutingError::Unroutable("alice".into()));
}

#[test]
fn routed_relay_set_add_merges_sources_per_url() {
    let mut routed = RoutedRelaySet::new();
    let url: RelayUrl = "wss://r.example".into();
    routed.add(url.clone(), RoutingSource::Hint);
    routed.add(
        url.clone(),
        RoutingSource::Nip65 {
            direction: Direction::Write,
        },
    );

    let sources = &routed.relays[&url];
    assert_eq!(sources.len(), 2);
    assert!(sources.contains(&RoutingSource::Hint));
    assert!(sources.contains(&RoutingSource::Nip65 {
        direction: Direction::Write
    }));
}

#[test]
fn routing_source_ordering_is_stable() {
    // Ord on RoutingSource is derived and load-bearing — the inner
    // BTreeSet of RoutedRelaySet relies on it. Smoke-check determinism.
    let mut sources: Vec<RoutingSource> = vec![
        RoutingSource::Indexer,
        RoutingSource::Hint,
        RoutingSource::Nip65 {
            direction: Direction::Read,
        },
        RoutingSource::AppRelay {
            mode: AppRelayMode::Fallback,
        },
    ];
    sources.sort();
    sources.dedup();
    assert_eq!(sources.len(), 4);
}
