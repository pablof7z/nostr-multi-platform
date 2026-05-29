//! V-75 per-lane RouteAttempt observability tests.
//!
//! Verifies that `GenericOutboxRouter` emits one `RouteAttempt` per lane that
//! ran in the generic algorithm, with the correct `lane` and `outcome`. The
//! primary scenario is "lanes 1–6 empty, Lane 7 (AppRelayFallback) fires" on
//! both publish and subscribe paths.
//!
//! Companion files:
//! - `tests.rs` — lanes 1, 6, 7 + explicit-targets shortcut + V-51 observer
//! - `tests_lanes.rs` — lanes 2/3/4/5 coverage
//! - `tests_v75.rs` — this file: per-lane RouteAttempt attribution (V-75)

use super::*;
use std::sync::{Arc, Mutex};

use nmp_core::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape};
use nmp_core::substrate::{
    BlockedRelaySet, LaneOutcome, MailboxCache, ParsedRelayList, RouteAttempt, RoutingLane,
    SessionKeySet,
};

use crate::InMemoryMailboxCache;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn pubkey() -> String {
    "alice".into()
}

fn unsigned_evt() -> UnsignedEvent {
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

fn ctx_app_only<'a>(
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
        explicit_targets: None,
    }
}

fn ctx_nip65_only<'a>(
    cache: &'a dyn MailboxCache,
    blocked: &'a BlockedRelaySet,
) -> RoutingContext<'a> {
    RoutingContext {
        active_account: None,
        session_keys: SessionKeySet::default(),
        mailbox_cache: cache,
        blocked_relays: blocked,
        explicit_targets: None,
    }
}

// ─── Capture observer ────────────────────────────────────────────────────────

/// Test observer that captures the full `PublishTrace` / `SubscriptionTrace`
/// including the V-75 `attempts` field.
#[derive(Default)]
struct AttemptCapture {
    pub publish_attempts: Mutex<Vec<Vec<RouteAttempt>>>,
    pub subscription_attempts: Mutex<Vec<Vec<RouteAttempt>>>,
}

impl RoutingTraceObserver for AttemptCapture {
    fn on_publish(&self, summary: PublishTrace, _routed: &RoutedRelaySet) {
        self.publish_attempts
            .lock()
            .unwrap()
            .push(summary.attempts);
    }
    fn on_subscription(&self, summary: SubscriptionTrace, _routed: &RoutedRelaySet) {
        self.subscription_attempts
            .lock()
            .unwrap()
            .push(summary.attempts);
    }
}

// ─── Publish path ─────────────────────────────────────────────────────────────

/// Core V-75 scenario: lanes 1–6 all empty, Lane 7 fires.
/// The trace MUST end with AppRelayFallback + Matched and all prior lanes
/// must appear as Empty.
#[test]
fn publish_lane7_fallback_traces_empty_lanes_then_app_relay_fallback() {
    let cache = InMemoryMailboxCache::new(); // empty NIP-65
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let c = ctx_app_only(&cache, &blocked, &app);

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    let r = router.route_publish(&unsigned_evt(), &c).unwrap();

    // Confirm Lane 7 actually resolved the app relay.
    assert!(
        r.relays
            .get(&"wss://app.example".to_string())
            .map(|s| s.iter().any(|src| matches!(
                src,
                RoutingSource::AppRelay { mode: AppRelayMode::Fallback }
            )))
            .unwrap_or(false),
        "AppRelay fallback must be in the resolved set"
    );

    let caps = obs.publish_attempts.lock().unwrap();
    assert_eq!(caps.len(), 1, "one publish trace");
    let attempts = &caps[0];

    // There must be at least one attempt.
    assert!(
        !attempts.is_empty(),
        "publish attempts must not be empty when observer is installed"
    );

    // The last attempt must be AppRelayFallback with Matched { count >= 1 }.
    let last = attempts.last().unwrap();
    assert_eq!(
        last.lane,
        RoutingLane::AppRelayFallback,
        "last attempt must be AppRelayFallback; got {:?}",
        last
    );
    assert!(
        matches!(last.outcome, LaneOutcome::Matched { count } if count >= 1),
        "AppRelayFallback outcome must be Matched(>=1); got {:?}",
        last.outcome
    );

    // All prior attempts must be Empty (no NIP-65, no hints, no UserConfigured
    // for lane 4 since active_account is None and pubkey != active).
    for a in attempts.iter().take(attempts.len() - 1) {
        assert_eq!(
            a.outcome,
            LaneOutcome::Empty,
            "lane {:?} must be Empty before AppRelayFallback; got {:?}",
            a.lane,
            a.outcome
        );
    }
}

/// When Lane 1 (NIP-65 write) resolves, AppRelayFallback must NOT appear in
/// the attempts (Lane 7 only fires when all prior lanes are empty).
#[test]
fn publish_lane1_match_no_app_relay_fallback_attempt() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert(pubkey(), ParsedRelayList {
        write: vec!["wss://w.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let c = ctx_app_only(&*cache, &blocked, &app);

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router.route_publish(&unsigned_evt(), &c).unwrap();

    let caps = obs.publish_attempts.lock().unwrap();
    let attempts = &caps[0];

    // Lane 1 (Nip65) must be Matched.
    let nip65 = attempts.iter().find(|a| a.lane == RoutingLane::Nip65);
    assert!(
        matches!(nip65, Some(RouteAttempt { outcome: LaneOutcome::Matched { .. }, .. })),
        "Lane 1 must be Matched; got {nip65:?}"
    );

    // AppRelayFallback must NOT appear.
    let fallback = attempts.iter().find(|a| a.lane == RoutingLane::AppRelayFallback);
    assert!(
        fallback.is_none(),
        "AppRelayFallback must not appear when lane 1 resolved; got {fallback:?}"
    );
}

/// When no observer is installed, `attempts` is never populated. This is a
/// D8 contract test — zero allocation on the no-observer path.
#[test]
fn publish_no_observer_no_attempts_accumulated() {
    // We can only verify this indirectly: the route call must succeed
    // without the router allocating or storing attempts anywhere.
    // We do NOT install an observer; the router must still work correctly.
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert(pubkey(), ParsedRelayList {
        write: vec!["wss://w.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let app: Vec<String> = vec![];
    let c = ctx_nip65_only(&*cache, &blocked);

    let router = GenericOutboxRouter::new(); // no observer
    let r = router.route_publish(&unsigned_evt(), &c).unwrap();
    assert!(r.urls().any(|u| u == "wss://w.example"));
}

/// When `explicit_targets` is set (lane 5 / ClassRouted), the generic algorithm
/// is skipped and `attempts` must be empty.
#[test]
fn publish_explicit_targets_produces_empty_attempts() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let explicit = vec!["wss://forced.example".to_string()];
    let app: Vec<String> = vec![];
    let c = RoutingContext {
        active_account: None,
        session_keys: SessionKeySet {
            app_relays: &app,
            ..SessionKeySet::default()
        },
        mailbox_cache: &cache,
        blocked_relays: &blocked,
        explicit_targets: Some(&explicit),
    };

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router.route_publish(&unsigned_evt(), &c).unwrap();

    let caps = obs.publish_attempts.lock().unwrap();
    assert_eq!(caps.len(), 1);
    assert!(
        caps[0].is_empty(),
        "explicit_targets path must produce empty attempts; got {:?}",
        caps[0]
    );
}

// ─── Subscribe path ───────────────────────────────────────────────────────────

/// Subscribe core V-75 scenario: lanes 1–6 empty, Lane 7 fires.
#[test]
fn subscribe_lane7_fallback_traces_empty_lanes_then_app_relay_fallback() {
    let cache = InMemoryMailboxCache::new(); // empty NIP-65
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let c = ctx_app_only(&cache, &blocked, &app);

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router.route_subscription(&interest_for(&["alice"]), &c).unwrap();

    let caps = obs.subscription_attempts.lock().unwrap();
    assert_eq!(caps.len(), 1);
    let attempts = &caps[0];

    assert!(
        !attempts.is_empty(),
        "subscribe attempts must not be empty when observer is installed"
    );

    // Last attempt must be AppRelayFallback + Matched.
    let last = attempts.last().unwrap();
    assert_eq!(
        last.lane,
        RoutingLane::AppRelayFallback,
        "last subscribe attempt must be AppRelayFallback; got {:?}",
        last
    );
    assert!(
        matches!(last.outcome, LaneOutcome::Matched { count } if count >= 1),
        "AppRelayFallback outcome must be Matched(>=1); got {:?}",
        last.outcome
    );

    // Prior attempts must be Empty.
    for a in attempts.iter().take(attempts.len() - 1) {
        assert_eq!(
            a.outcome,
            LaneOutcome::Empty,
            "lane {:?} must be Empty before AppRelayFallback (subscribe); got {:?}",
            a.lane,
            a.outcome
        );
    }
}

/// Subscribe explicit-targets path produces empty attempts.
#[test]
fn subscribe_explicit_targets_produces_empty_attempts() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let explicit = vec!["wss://override.example".to_string()];
    let app: Vec<String> = vec![];
    let c = RoutingContext {
        active_account: None,
        session_keys: SessionKeySet {
            app_relays: &app,
            ..SessionKeySet::default()
        },
        mailbox_cache: &cache,
        blocked_relays: &blocked,
        explicit_targets: Some(&explicit),
    };

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router.route_subscription(&interest_for(&["alice"]), &c).unwrap();

    let caps = obs.subscription_attempts.lock().unwrap();
    assert_eq!(caps.len(), 1);
    assert!(
        caps[0].is_empty(),
        "explicit_targets subscribe path must produce empty attempts; got {:?}",
        caps[0]
    );
}

/// Subscribe: Lane 1 (NIP-65 read) match means AppRelayFallback does NOT appear.
#[test]
fn subscribe_lane1_match_no_app_relay_fallback_attempt() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.upsert("alice".into(), ParsedRelayList {
        read: vec!["wss://r.example".into()],
        ..ParsedRelayList::default()
    });
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let c = ctx_app_only(&*cache, &blocked, &app);

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router.route_subscription(&interest_for(&["alice"]), &c).unwrap();

    let caps = obs.subscription_attempts.lock().unwrap();
    let attempts = &caps[0];

    let nip65 = attempts.iter().find(|a| a.lane == RoutingLane::Nip65);
    assert!(
        matches!(nip65, Some(RouteAttempt { outcome: LaneOutcome::Matched { .. }, .. })),
        "Lane 1 must be Matched; got {nip65:?}"
    );

    let fallback = attempts.iter().find(|a| a.lane == RoutingLane::AppRelayFallback);
    assert!(
        fallback.is_none(),
        "AppRelayFallback must not appear when lane 1 resolved; got {fallback:?}"
    );
}

// ─── Attempt ordering (lane-order invariant) ─────────────────────────────────

/// Publish attempts must be emitted in lane order: Nip65 before Hint before
/// UserConfigured before (optionally Indexer) before AppRelayFallback.
#[test]
fn publish_attempts_are_emitted_in_lane_order() {
    let cache = InMemoryMailboxCache::new(); // all generic lanes empty
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let c = ctx_app_only(&cache, &blocked, &app);

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router.route_publish(&unsigned_evt(), &c).unwrap();

    let caps = obs.publish_attempts.lock().unwrap();
    let attempts = &caps[0];

    // Build the expected order: Nip65, Hint, UserConfigured, [Indexer for
    // discovery kinds], AppRelayFallback. For kind:1 (non-discovery) there
    // is no Indexer entry.
    let lanes: Vec<RoutingLane> = attempts.iter().map(|a| a.lane).collect();
    let nip65_pos = lanes.iter().position(|l| *l == RoutingLane::Nip65);
    let hint_pos = lanes.iter().position(|l| *l == RoutingLane::Hint);
    let uc_pos = lanes.iter().position(|l| *l == RoutingLane::UserConfigured);
    let fallback_pos = lanes.iter().position(|l| *l == RoutingLane::AppRelayFallback);

    // All must be present for kind:1.
    assert!(nip65_pos.is_some(), "Nip65 attempt missing; got {lanes:?}");
    assert!(hint_pos.is_some(), "Hint attempt missing; got {lanes:?}");
    assert!(uc_pos.is_some(), "UserConfigured attempt missing; got {lanes:?}");
    assert!(fallback_pos.is_some(), "AppRelayFallback attempt missing; got {lanes:?}");

    // Order: Nip65 < Hint < UserConfigured < AppRelayFallback.
    assert!(
        nip65_pos.unwrap() < hint_pos.unwrap(),
        "Nip65 must precede Hint"
    );
    assert!(
        hint_pos.unwrap() < uc_pos.unwrap(),
        "Hint must precede UserConfigured"
    );
    assert!(
        uc_pos.unwrap() < fallback_pos.unwrap(),
        "UserConfigured must precede AppRelayFallback"
    );
}
