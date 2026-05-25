//! Lane-coverage tests for `GenericOutboxRouter` lanes 2/3/4/5.
//!
//! Sibling of `router/tests.rs`. The original test file covers the lanes
//! that were complete at step 2 (lanes 1, 6, 7 + the `explicit_targets`
//! shortcut + the trace observer). This file covers the lanes filled in
//! when the TODOs at `router.rs` lanes 2–5 were resolved:
//!
//! - Lane 2 — Hint (`evt.tags` e/p/a/q position 2; `interest.hints`
//!   carrying `HintSource::EventTag`).
//! - Lane 3 — Provenance (`interest.hints` carrying
//!   `HintSource::Provenance`).
//! - Lane 4 — UserConfigured (active-account read/write).
//! - Lane 5 — ClassRouted (explicit-targets attribution refined to the
//!   right `EventClass` for the publish kind).
//!
//! Split out to keep both `tests.rs` and this file under the 500-LOC
//! ceiling.

use super::*;
use std::sync::Arc;

use nmp_core::planner::{
    HintSource, InterestId, InterestLifecycle, InterestScope, InterestShape, RelayHint,
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

fn ctx_with_active<'a>(
    cache: &'a dyn MailboxCache,
    blocked: &'a BlockedRelaySet,
    active: &'a String,
    active_read: &'a [String],
    active_write: &'a [String],
) -> RoutingContext<'a> {
    RoutingContext {
        active_account: Some(active),
        session_keys: SessionKeySet {
            active_read,
            active_write,
            ..SessionKeySet::default()
        },
        mailbox_cache: cache,
        blocked_relays: blocked,
        explicit_targets: None,
    }
}

// ─── Lane 2 (Hint) — relay-hint tags on the event ──────────────────────

#[test]
fn publish_lane2_lifts_relay_hints_from_e_p_a_q_tags() {
    // Empty NIP-65, no app-relay — only the hint lane can resolve. Each
    // tag carries a different hint URL; the resolved set must contain
    // all four and attribute each one to `RoutingSource::Hint`.
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let c = ctx(&cache, &blocked, None, &app);

    let evt = UnsignedEvent {
        tags: vec![
            vec!["e".into(), "id1".into(), "wss://e-hint.example".into()],
            vec!["p".into(), "pk1".into(), "wss://p-hint.example".into()],
            vec!["a".into(), "30023:pk:d".into(), "wss://a-hint.example".into()],
            vec!["q".into(), "id2".into(), "wss://q-hint.example".into()],
        ],
        ..unsigned()
    };
    let router = GenericOutboxRouter::new();
    let r = router.route_publish(&evt, &c).unwrap();
    let urls: std::collections::BTreeSet<&String> = r.urls().collect();
    for want in [
        "wss://e-hint.example",
        "wss://p-hint.example",
        "wss://a-hint.example",
        "wss://q-hint.example",
    ] {
        let s = want.to_string();
        assert!(urls.contains(&s), "lane 2 must include {want}; got {urls:?}");
        assert!(
            r.relays[&s].contains(&RoutingSource::Hint),
            "{want} must carry RoutingSource::Hint; got {:?}",
            r.relays[&s]
        );
    }
    // Hints resolved a set, so lane 7 (AppRelay fallback) MUST NOT fire.
    assert!(
        !urls.contains(&"wss://app.example".to_string()),
        "AppRelay fallback must not stack on top of lane 2"
    );
}

#[test]
fn publish_lane2_stacks_with_lane1_for_same_url() {
    // A relay appearing in BOTH NIP-65 write set AND the e-tag hint slot
    // must carry both RoutingSource lanes in its inner set.
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert(pubkey(), ParsedRelayList {
        write: vec!["wss://shared.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&*cache, &blocked, None, &app);
    let evt = UnsignedEvent {
        tags: vec![vec!["e".into(), "id".into(), "wss://shared.example".into()]],
        ..unsigned()
    };
    let router = GenericOutboxRouter::new();
    let r = router.route_publish(&evt, &c).unwrap();
    let url = "wss://shared.example".to_string();
    let sources = &r.relays[&url];
    assert!(sources.contains(&RoutingSource::Hint));
    assert!(sources.contains(&RoutingSource::Nip65 { direction: Direction::Write }));
}

#[test]
fn publish_lane2_skips_empty_hint_slots_and_unknown_tag_keys() {
    // NIP-10 marker form `["e", id, "", "reply"]` leaves position 2 empty
    // — must be skipped. A `t` (hashtag) tag with a URL-shaped third
    // column must also be ignored (lane 2 only honours e/p/a/q).
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&cache, &blocked, None, &app);
    let evt = UnsignedEvent {
        tags: vec![
            vec!["e".into(), "id".into(), "".into(), "reply".into()],
            vec!["t".into(), "tag".into(), "wss://not-a-hint.example".into()],
        ],
        ..unsigned()
    };
    let router = GenericOutboxRouter::new();
    let err = router.route_publish(&evt, &c).unwrap_err();
    assert_eq!(err, RoutingError::Unroutable(pubkey()));
}

#[test]
fn publish_lane2_blocked_relay_post_filter_applies() {
    let cache = InMemoryMailboxCache::new();
    let mut blocked = BlockedRelaySet::new();
    blocked.insert("wss://bad.example".into());
    let app: Vec<String> = vec![];
    let c = ctx(&cache, &blocked, None, &app);
    let evt = UnsignedEvent {
        tags: vec![vec!["e".into(), "id".into(), "wss://bad.example".into()]],
        ..unsigned()
    };
    let router = GenericOutboxRouter::new();
    // Blocked hint silently dropped → no lane resolved → Unroutable.
    let err = router.route_publish(&evt, &c).unwrap_err();
    assert_eq!(err, RoutingError::Unroutable(pubkey()));
}

#[test]
fn subscribe_lane2_lifts_event_tag_hints_from_interest() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&cache, &blocked, None, &app);
    let mut interest = interest_for(&["alice"]);
    interest.hints.push(RelayHint {
        url: "wss://hint-from-tag.example".into(),
        source: HintSource::EventTag {
            event_id: "evt1".into(),
            tag: "e".into(),
            position: 2,
        },
    });
    let router = GenericOutboxRouter::new();
    let r = router.route_subscription(&interest, &c).unwrap();
    let url = "wss://hint-from-tag.example".to_string();
    assert!(r.urls().any(|u| u == &url));
    assert!(r.relays[&url].contains(&RoutingSource::Hint));
}

// ─── Lane 3 (Provenance) — subscribe-only ──────────────────────────────

#[test]
fn subscribe_lane3_lifts_provenance_hints_from_interest() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&cache, &blocked, None, &app);
    let mut interest = interest_for(&["alice"]);
    interest.hints.push(RelayHint {
        url: "wss://prov.example".into(),
        source: HintSource::Provenance { event_id: "seen-here".into() },
    });
    let router = GenericOutboxRouter::new();
    let r = router.route_subscription(&interest, &c).unwrap();
    let url = "wss://prov.example".to_string();
    assert!(r.urls().any(|u| u == &url));
    assert!(r.relays[&url].contains(&RoutingSource::Provenance));
}

#[test]
fn subscribe_lane3_distinct_from_lane2_attribution() {
    // Two hints on the same interest — one EventTag, one Provenance —
    // attribute to distinct lanes.
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx(&cache, &blocked, None, &app);
    let mut interest = interest_for(&["alice"]);
    interest.hints.push(RelayHint {
        url: "wss://tag.example".into(),
        source: HintSource::EventTag {
            event_id: "x".into(),
            tag: "e".into(),
            position: 2,
        },
    });
    interest.hints.push(RelayHint {
        url: "wss://prov.example".into(),
        source: HintSource::Provenance { event_id: "y".into() },
    });
    let router = GenericOutboxRouter::new();
    let r = router.route_subscription(&interest, &c).unwrap();
    assert!(r.relays[&"wss://tag.example".to_string()].contains(&RoutingSource::Hint));
    assert!(
        r.relays[&"wss://prov.example".to_string()].contains(&RoutingSource::Provenance)
    );
}

// ─── Lane 4 (UserConfigured) — active-account read/write ───────────────

#[test]
fn publish_lane4_active_account_write_fires_for_self_publish() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let active = pubkey();
    let aw = vec!["wss://my-write.example".to_string()];
    let c = ctx_with_active(&cache, &blocked, &active, &[], &aw);
    let router = GenericOutboxRouter::new();
    let r = router.route_publish(&unsigned(), &c).unwrap();
    let url = "wss://my-write.example".to_string();
    assert!(r.urls().any(|u| u == &url));
    assert!(r.relays[&url].contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::ActiveAccountWrite,
    )));
}

#[test]
fn publish_lane4_silent_when_evt_pubkey_differs_from_active() {
    // Publishing as bob while alice is the active account: lane 4 MUST
    // NOT add alice's active_write set to bob's event.
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let active = String::from("alice");
    let aw = vec!["wss://alice-write.example".to_string()];
    let c = ctx_with_active(&cache, &blocked, &active, &[], &aw);
    let router = GenericOutboxRouter::new();
    let evt = UnsignedEvent { pubkey: "bob".into(), ..unsigned() };
    // No other lane fires either; Unroutable.
    let err = router.route_publish(&evt, &c).unwrap_err();
    assert_eq!(err, RoutingError::Unroutable("bob".into()));
}

#[test]
fn subscribe_lane4_active_account_read_fires_when_active_in_authors() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let active = String::from("alice");
    let ar = vec!["wss://my-read.example".to_string()];
    let c = ctx_with_active(&cache, &blocked, &active, &ar, &[]);
    let router = GenericOutboxRouter::new();
    let r = router
        .route_subscription(&interest_for(&["alice", "bob"]), &c)
        .unwrap();
    let url = "wss://my-read.example".to_string();
    assert!(r.urls().any(|u| u == &url));
    assert!(r.relays[&url].contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::ActiveAccountRead,
    )));
}

#[test]
fn subscribe_lane4_silent_when_active_not_in_authors() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let active = String::from("alice");
    let ar = vec!["wss://my-read.example".to_string()];
    let c = ctx_with_active(&cache, &blocked, &active, &ar, &[]);
    let router = GenericOutboxRouter::new();
    // bob only — alice not in scope.
    let err = router
        .route_subscription(&interest_for(&["bob"]), &c)
        .unwrap_err();
    assert_eq!(err, RoutingError::Unroutable("bob".into()));
}

#[test]
fn subscribe_lane4_fires_for_authorless_wildcard_interest() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let active = String::from("alice");
    let ar = vec!["wss://my-read.example".to_string()];
    let c = ctx_with_active(&cache, &blocked, &active, &ar, &[]);
    let router = GenericOutboxRouter::new();
    let r = router.route_subscription(&interest_for(&[]), &c).unwrap();
    let url = "wss://my-read.example".to_string();
    assert!(r.urls().any(|u| u == &url));
    assert!(r.relays[&url].contains(&RoutingSource::UserConfigured(
        UserConfiguredCategory::ActiveAccountRead,
    )));
}

// ─── Lane 5 (ClassRouted) — explicit_targets carries the right class ──

#[test]
fn explicit_publish_lane5_classifies_wiki_kinds() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let explicit = vec!["wss://wiki-relay.example".to_string()];
    let app: Vec<String> = vec![];
    let router = GenericOutboxRouter::new();
    for kind in [818u32, 30_818, 30_819] {
        let c = ctx(&cache, &blocked, Some(&explicit), &app);
        let evt = UnsignedEvent { kind, ..unsigned() };
        let r = router.route_publish(&evt, &c).unwrap();
        let url = "wss://wiki-relay.example".to_string();
        let sources = &r.relays[&url];
        assert!(
            sources.iter().any(|s| matches!(
                s,
                RoutingSource::ClassRouted {
                    class: EventClass::Wiki,
                    via: ClassRoutingPath::Explicit,
                }
            )),
            "kind {kind} must attribute to ClassRouted{{Wiki, Explicit}}; got {sources:?}"
        );
    }
}

#[test]
fn explicit_publish_lane5_classifies_draft_kinds() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let explicit = vec!["wss://drafts.example".to_string()];
    let app: Vec<String> = vec![];
    let router = GenericOutboxRouter::new();
    for kind in [1234u32, 31_234] {
        let c = ctx(&cache, &blocked, Some(&explicit), &app);
        let evt = UnsignedEvent { kind, ..unsigned() };
        let r = router.route_publish(&evt, &c).unwrap();
        let url = "wss://drafts.example".to_string();
        let sources = &r.relays[&url];
        assert!(
            sources.iter().any(|s| matches!(
                s,
                RoutingSource::ClassRouted {
                    class: EventClass::Draft,
                    via: ClassRoutingPath::Explicit,
                }
            )),
            "kind {kind} must attribute to ClassRouted{{Draft, Explicit}}; got {sources:?}"
        );
    }
}

#[test]
fn explicit_publish_lane5_falls_back_to_other_for_unclassified_kinds() {
    // kind:1 — no class binding in the router. Must attribute to
    // `EventClass::Other("explicit")` (preserves the pre-existing label
    // so the routing-trace JSON stays stable for these kinds).
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let explicit = vec!["wss://forced.example".to_string()];
    let app: Vec<String> = vec![];
    let c = ctx(&cache, &blocked, Some(&explicit), &app);
    let router = GenericOutboxRouter::new();
    let r = router.route_publish(&unsigned(), &c).unwrap();
    let url = "wss://forced.example".to_string();
    let sources = &r.relays[&url];
    let other = sources.iter().find(|s| matches!(
        s,
        RoutingSource::ClassRouted { via: ClassRoutingPath::Explicit, .. }
    ));
    match other {
        Some(RoutingSource::ClassRouted { class, .. }) => match class {
            EventClass::Other(name) => assert_eq!(name, "explicit"),
            _ => panic!("kind:1 must classify to Other(\"explicit\"), got {class:?}"),
        },
        _ => panic!("ClassRouted source missing: {sources:?}"),
    }
}

#[test]
fn explicit_publish_lane5_blocked_relay_post_filter_applies() {
    let cache = InMemoryMailboxCache::new();
    let mut blocked = BlockedRelaySet::new();
    blocked.insert("wss://wiki-bad.example".into());
    let explicit = vec![
        "wss://wiki-bad.example".to_string(),
        "wss://wiki-ok.example".to_string(),
    ];
    let app: Vec<String> = vec![];
    let c = ctx(&cache, &blocked, Some(&explicit), &app);
    let router = GenericOutboxRouter::new();
    let evt = UnsignedEvent { kind: 30_818, ..unsigned() };
    let r = router.route_publish(&evt, &c).unwrap();
    let urls: Vec<&String> = r.urls().collect();
    assert_eq!(urls, vec![&"wss://wiki-ok.example".to_string()]);
}
