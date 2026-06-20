use crate::{
    compiler::{InMemoryMailboxCache, SubscriptionCompiler},
    interest::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest},
    plan::{RoutingSource, UserConfiguredCategory},
};
use std::collections::{BTreeMap, BTreeSet};

fn hashtag_interest(id: u64, tag: &str) -> LogicalInterest {
    let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut vals = BTreeSet::new();
    vals.insert(tag.to_string());
    tags.insert("t".to_string(), vals);
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [1u32].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

fn no_author_kind_interest(id: u64, kind: u32) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [kind].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// active_account ∪ app_relays — both lanes recorded on the union URL.
#[test]
fn case_d_unions_active_account_with_app_relays() {
    let cache = InMemoryMailboxCache::new();
    let aar = vec!["wss://read-1".to_string(), "wss://shared".to_string()];
    let app = vec!["wss://app".to_string(), "wss://shared".to_string()];
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &aar, &app);

    let plan = compiler
        .compile(&[hashtag_interest(1, "nostr")])
        .expect("compile");

    // AccountRead-only URL.
    let read1 = plan.per_relay.get("wss://read-1").expect("read-1");
    assert!(read1.role_tags.contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::AccountRead
    )));
    assert!(!read1.role_tags.contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::AppRelay
    )));

    // AppRelay-only URL.
    let app_p = plan.per_relay.get("wss://app").expect("app");
    assert!(app_p.role_tags.contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::AppRelay
    )));

    // Both lanes on shared URL.
    let shared = plan.per_relay.get("wss://shared").expect("shared");
    assert!(shared.role_tags.contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::AccountRead
    )));
    assert!(shared.role_tags.contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::AppRelay
    )));
}

/// Cold-start: both active_account and app_relays empty → fall through
/// to indexer as a last-resort landing pad (kernel discovery REQs).
#[test]
fn case_d_cold_start_falls_through_to_indexer() {
    let cache = InMemoryMailboxCache::new();
    let indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &[]);

    let plan = compiler
        .compile(&[hashtag_interest(1, "nostr")])
        .expect("compile");

    let ix = plan
        .per_relay
        .get("wss://purplepag.es")
        .expect("indexer fallback");
    assert!(ix.role_tags.contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::Indexer
    )));
}

/// app_relays alone (no active_account) → routes to app_relays without
/// touching the indexer set.
#[test]
fn case_d_app_relays_alone_skips_indexer() {
    let cache = InMemoryMailboxCache::new();
    let indexer = vec!["wss://purplepag.es".to_string()];
    let app = vec!["wss://app".to_string()];
    let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

    let plan = compiler
        .compile(&[hashtag_interest(1, "nostr")])
        .expect("compile");

    assert!(plan.per_relay.contains_key("wss://app"));
    assert!(
        !plan.per_relay.contains_key("wss://purplepag.es"),
        "indexer must NOT be touched when app_relays carry the firehose"
    );
}

#[test]
fn case_d_relay_set_feed_keeps_no_authors_filter() {
    let cache = InMemoryMailboxCache::new();
    let indexer = vec!["wss://purplepag.es".to_string()];
    let app = vec![
        "wss://relay-a.example".to_string(),
        "wss://relay-b.example".to_string(),
        "wss://relay-c.example".to_string(),
    ];
    let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

    let plan = compiler
        .compile(&[no_author_kind_interest(1, 30_023)])
        .expect("compile");

    assert_eq!(
        plan.per_relay.keys().cloned().collect::<BTreeSet<_>>(),
        app.into_iter().collect::<BTreeSet<_>>(),
        "relay-set style feeds should target app relays only, not indexers"
    );
    for relay in plan.per_relay.values() {
        assert_eq!(relay.sub_shapes.len(), 1);
        let shape = &relay.sub_shapes[0].shape;
        assert!(
            shape.authors.is_empty(),
            "relay-set feeds must not gain an authors filter"
        );
        assert_eq!(shape.kinds, [30_023u32].into_iter().collect());
    }
}

fn hex(byte: &str) -> String {
    byte.repeat(32)
}

fn discovery_oneshot_ids(id: u64, event_ids: &[&str]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            event_ids: event_ids.iter().map(|s| hex(s)).collect(),
            limit: Some(event_ids.len() as u32),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

#[test]
fn pd033c_event_ids_oneshot_global_routes_to_bootstrap_content() {
    let cache = InMemoryMailboxCache::new();
    let indexer = vec!["wss://purplepag.es".to_string()];
    let bootstrap = vec!["wss://relay.primal.net".to_string()];
    let aar = vec!["wss://user-read.example".to_string()];
    let app = vec!["wss://user-app.example".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &indexer,
        &aar,
        &app,
        &bootstrap,
        /* bootstrap_indexer = */ &[],
    );

    let plan = compiler
        .compile(&[discovery_oneshot_ids(1, &["aa", "bb"])])
        .expect("compile");

    let landed = plan
        .per_relay
        .get("wss://relay.primal.net")
        .expect("bootstrap content relay must carry the discovery REQ");
    assert!(landed.role_tags.contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::Bootstrap,
    )));
    assert!(
        !plan.per_relay.contains_key("wss://purplepag.es"),
        "event_ids discovery must NOT land on the indexer lane"
    );
    assert!(!plan.per_relay.contains_key("wss://user-read.example"));
    assert!(!plan.per_relay.contains_key("wss://user-app.example"));
    assert_eq!(plan.per_relay.len(), 1);
}

#[test]
fn pd033c_event_ids_oneshot_with_empty_bootstrap_falls_through() {
    let cache = InMemoryMailboxCache::new();
    let indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &indexer,
        &[],
        &[],
        /* bootstrap_content = */ &[],
        /* bootstrap_indexer = */ &[],
    );

    let plan = compiler
        .compile(&[discovery_oneshot_ids(1, &["aa"])])
        .expect("compile");

    let ix = plan
        .per_relay
        .get("wss://purplepag.es")
        .expect("indexer fallback still applies when bootstrap is empty");
    assert!(ix.role_tags.contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::Indexer
    )));
}

#[test]
fn pd033c_tailing_event_ids_does_not_trigger_bootstrap_gate() {
    let cache = InMemoryMailboxCache::new();
    let indexer = vec!["wss://purplepag.es".to_string()];
    let bootstrap = vec!["wss://relay.primal.net".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &indexer,
        &[],
        &[],
        &bootstrap,
        /* bootstrap_indexer = */ &[],
    );

    let mut interest = discovery_oneshot_ids(1, &["aa"]);
    interest.lifecycle = InterestLifecycle::Tailing;
    let plan = compiler.compile(&[interest]).expect("compile");

    assert!(
        !plan.per_relay.contains_key("wss://relay.primal.net"),
        "Tailing event_ids must NOT route to bootstrap content relays"
    );
    assert!(plan.per_relay.contains_key("wss://purplepag.es"));
}

#[test]
fn pd033c_account_scoped_event_ids_does_not_trigger_bootstrap_gate() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap = vec!["wss://relay.primal.net".to_string()];
    let indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &indexer,
        &[],
        &[],
        &bootstrap,
        /* bootstrap_indexer = */ &[],
    );

    let mut interest = discovery_oneshot_ids(1, &["aa"]);
    interest.scope = InterestScope::Account(hex("cc"));
    let plan = compiler.compile(&[interest]).expect("compile");

    assert!(
        !plan.per_relay.contains_key("wss://relay.primal.net"),
        "Account-scoped event_ids must NOT route to bootstrap content relays"
    );
}

#[test]
fn pd033c_oneshot_global_without_event_ids_does_not_trigger_bootstrap_gate() {
    let cache = InMemoryMailboxCache::new();
    let bootstrap = vec!["wss://relay.primal.net".to_string()];
    let indexer = vec!["wss://purplepag.es".to_string()];
    let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &indexer,
        &[],
        &[],
        &bootstrap,
        /* bootstrap_indexer = */ &[],
    );

    let mut interest = hashtag_interest(1, "nostr");
    interest.lifecycle = InterestLifecycle::OneShot;
    let plan = compiler.compile(&[interest]).expect("compile");

    assert!(
        !plan.per_relay.contains_key("wss://relay.primal.net"),
        "OneShot+Global without event_ids must NOT route to bootstrap content"
    );
}

#[test]
fn pd033c_bootstrap_toggle_does_not_change_plan_id() {
    let cache = InMemoryMailboxCache::new();
    let interests = [discovery_oneshot_ids(1, &["aa"])];

    let bootstrap_set = vec!["wss://relay.primal.net".to_string()];
    let no_bootstrap = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        /* bootstrap_content = */ &[],
        /* bootstrap_indexer = */ &[],
    );
    let with_bootstrap = SubscriptionCompiler::with_relays_and_bootstrap(
        &cache,
        &[],
        &[],
        &[],
        &bootstrap_set,
        /* bootstrap_indexer = */ &[],
    );

    let plan_without = no_bootstrap.compile(&interests).expect("compile");
    let plan_with = with_bootstrap.compile(&interests).expect("compile");
    assert!(plan_without.per_relay.is_empty());
    assert!(plan_with.per_relay.contains_key("wss://relay.primal.net"));
    assert_eq!(
        plan_without.plan_id, plan_with.plan_id,
        "bootstrap_content_relays must be excluded from compute_plan_id \
         (matches app_relays treatment — see compile_with_context Stage 4)"
    );
}
