//! V-75 subscribe-path `RouteAttempt` scenarios: mirrors the publish-path
//! Lane-7 fallback core scenario and the Lane-1-match suppresses-fallback
//! contract on `route_subscription`.

use std::sync::Arc;

use super::fixtures::{ctx_app_only, interest_for, AttemptCapture};
use crate::router::*;
use crate::InMemoryMailboxCache;
use nmp_core::substrate::{
    BlockedRelaySet, LaneOutcome, ParsedRelayList, RouteAttempt, RoutingLane,
};

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
    router
        .route_subscription(&interest_for(&["alice"]), &c)
        .unwrap();

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

/// Subscribe: Lane 1 (NIP-65 write) match means AppRelayFallback does NOT appear.
#[test]
fn subscribe_lane1_match_no_app_relay_fallback_attempt() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.fixture_upsert(
        "alice".into(),
        ParsedRelayList {
            write: vec!["wss://r.example".into()],
            ..ParsedRelayList::default()
        },
    );
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let c = ctx_app_only(&*cache, &blocked, &app);

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router
        .route_subscription(&interest_for(&["alice"]), &c)
        .unwrap();

    let caps = obs.subscription_attempts.lock().unwrap();
    let attempts = &caps[0];

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

    let fallback = attempts
        .iter()
        .find(|a| a.lane == RoutingLane::AppRelayFallback);
    assert!(
        fallback.is_none(),
        "AppRelayFallback must not appear when lane 1 resolved; got {fallback:?}"
    );
}
