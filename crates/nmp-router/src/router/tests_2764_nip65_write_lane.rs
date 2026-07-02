//! #2764 regression tests — subscription lane-1 must use the author's NIP-65
//! WRITE/outbox set, never their READ/inbox set: you fetch someone's notes
//! from where they WRITE (NIP-65), not where they read. Split out of
//! `tests.rs` to stay under the 500-LOC file-size cap.

use super::*;
use std::sync::Arc;

use nmp_core::substrate::{
    BlockedRelaySet, Direction, MailboxCache, ParsedRelayList, RoutingSource, SessionKeySet,
};
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};

use crate::InMemoryMailboxCache;

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
    }
}

#[test]
fn subscribe_lane1_uses_write_not_read() {
    // Seed one author with three DISTINCT relay sets so a read/write mix-up
    // is structurally observable rather than accidentally passing on
    // overlapping fixtures.
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.fixture_upsert(
        "alice".into(),
        ParsedRelayList {
            read: vec!["wss://alice-read-only.example".into()],
            write: vec!["wss://alice-write-only.example".into()],
            both: vec!["wss://alice-both.example".into()],
        },
    );
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&*cache, &blocked, &app);

    let router = GenericOutboxRouter::new();
    let r = router
        .route_subscription(&interest_for(&["alice"]), &c)
        .unwrap();
    let urls: std::collections::BTreeSet<&String> = r.urls().collect();

    assert!(
        urls.contains(&"wss://alice-write-only.example".to_string()),
        "lane 1 must resolve the author's write relay — got {urls:?}"
    );
    assert!(
        urls.contains(&"wss://alice-both.example".to_string()),
        "lane 1 must resolve the author's both relay — got {urls:?}"
    );
    assert!(
        !urls.contains(&"wss://alice-read-only.example".to_string()),
        "lane 1 must NOT resolve the author's read-only relay (NIP-65 backwards) — got {urls:?}"
    );

    let sources = r
        .relays
        .get(&"wss://alice-write-only.example".to_string())
        .unwrap();
    assert!(sources.contains(&RoutingSource::Nip65 {
        direction: Direction::Write
    }));
}

/// Case-A parity proof: the router's lane-1 output for a single author must
/// equal `nmp-planner`'s `SubscriptionCompiler` (Case A / `outbox_relays()`)
/// output for the same mailbox snapshot — the planner is the live
/// production subscription-routing path (crate-boundaries.md §5); this test
/// pins the router's generic reference implementation to it.
#[test]
fn subscribe_lane1_parity_with_planner_case_a() {
    const READ_ONLY: &str = "wss://alice-read.example";
    const WRITE_ONLY: &str = "wss://alice-write.example";
    const BOTH: &str = "wss://alice-both.example";

    // Router side.
    let router_cache = Arc::new(InMemoryMailboxCache::new());
    router_cache.fixture_upsert(
        "alice".into(),
        ParsedRelayList {
            read: vec![READ_ONLY.into()],
            write: vec![WRITE_ONLY.into()],
            both: vec![BOTH.into()],
        },
    );
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&*router_cache, &blocked, &app);
    let router = GenericOutboxRouter::new();
    let routed = router
        .route_subscription(&interest_for(&["alice"]), &c)
        .unwrap();
    let router_urls: std::collections::BTreeSet<String> = routed.urls().cloned().collect();

    // Planner side — same mailbox snapshot compiled through Case A
    // (`case_a_authors::route` → `outbox_relays()` = write ∪ both).
    let mut planner_cache = nmp_planner::InMemoryMailboxCache::new();
    planner_cache.put(
        "alice".to_string(),
        nmp_planner::MailboxSnapshot {
            read_relays: vec![READ_ONLY.to_string()],
            write_relays: vec![WRITE_ONLY.to_string()],
            both_relays: vec![BOTH.to_string()],
        },
    );
    let compiler = nmp_planner::SubscriptionCompiler::new(&planner_cache, &[]);
    let plan = compiler
        .compile(&[interest_for(&["alice"])])
        .expect("planner compile must succeed");
    let planner_urls: std::collections::BTreeSet<String> = plan.per_relay.keys().cloned().collect();

    assert_eq!(
        router_urls, planner_urls,
        "router lane-1 output must match the planner's Case-A outbox_relays() set"
    );
    assert!(router_urls.contains(WRITE_ONLY) && router_urls.contains(BOTH));
    assert!(!router_urls.contains(READ_ONLY));
}
