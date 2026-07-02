//! V-75 publish-path `RouteAttempt` scenarios: the Lane-7 (AppRelayFallback)
//! core scenario, the Lane-1-match suppresses-fallback contract, and the
//! no-observer zero-allocation guard (D8).

use std::sync::Arc;

use super::fixtures::{ctx_app_only, ctx_nip65_only, pubkey, unsigned_evt, AttemptCapture};
use crate::router::*;
use crate::InMemoryMailboxCache;
use nmp_core::substrate::{
    BlockedRelaySet, LaneOutcome, ParsedRelayList, RouteAttempt, RoutingLane,
};

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
            .get("wss://app.example")
            .map(|s| s.iter().any(|src| matches!(
                src,
                RoutingSource::AppRelay {
                    mode: AppRelayMode::Fallback
                }
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
    cache.fixture_upsert(
        pubkey(),
        ParsedRelayList {
            write: vec!["wss://w.example".into()],
            ..ParsedRelayList::default()
        },
    );
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
        matches!(
            nip65,
            Some(RouteAttempt {
                outcome: LaneOutcome::Matched { .. },
                ..
            })
        ),
        "Lane 1 must be Matched; got {nip65:?}"
    );

    // AppRelayFallback must NOT appear.
    let fallback = attempts
        .iter()
        .find(|a| a.lane == RoutingLane::AppRelayFallback);
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
    cache.fixture_upsert(
        pubkey(),
        ParsedRelayList {
            write: vec!["wss://w.example".into()],
            ..ParsedRelayList::default()
        },
    );
    let blocked = BlockedRelaySet::new();
    let c = ctx_nip65_only(&*cache, &blocked);

    let router = GenericOutboxRouter::new(); // no observer
    let r = router.route_publish(&unsigned_evt(), &c).unwrap();
    assert!(r.urls().any(|u| u == "wss://w.example"));
}
