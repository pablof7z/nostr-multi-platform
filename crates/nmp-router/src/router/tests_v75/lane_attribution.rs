//! V-75 per-lane emission invariants: attempts are emitted in lane order,
//! only-applicable lanes emit at all (no `UserConfigured` without an active
//! account), and the admissible-count fix that lets the Hint lane report
//! `Matched` even when its relay is already present from the NIP-65 lane.

use std::sync::Arc;

use super::fixtures::{ctx_app_only, ctx_nip65_only, pubkey, unsigned_evt, AttemptCapture};
use crate::router::*;
use crate::InMemoryMailboxCache;
use nmp_core::substrate::{
    BlockedRelaySet, LaneOutcome, ParsedRelayList, RouteAttempt, RoutingLane, SessionKeySet,
};

/// Publish attempts are emitted in lane order: Nip65 before Hint before
/// AppRelayFallback. Lane 4 (UserConfigured) and Lane 6 (Indexer) are only
/// emitted when applicable (active account in scope, or discovery kind) and
/// appear between Hint and AppRelayFallback when they do.
///
/// This test uses active_account = pubkey() so that Lane 4 fires (empty
/// active_write → UserConfigured Empty), confirming the ordering invariant.
#[test]
fn publish_attempts_are_emitted_in_lane_order() {
    let cache = InMemoryMailboxCache::new(); // empty NIP-65
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let active = pubkey(); // active_account == evt.pubkey → Lane 4 fires
    let active_write: Vec<String> = vec![]; // empty → UserConfigured Empty
    let c = RoutingContext {
        active_account: Some(&active),
        session_keys: SessionKeySet {
            app_relays: &app,
            active_write: &active_write,
            ..SessionKeySet::default()
        },
        mailbox_cache: &cache,
        blocked_relays: &blocked,
    };

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router.route_publish(&unsigned_evt(), &c).unwrap();

    let caps = obs.publish_attempts.lock().unwrap();
    let attempts = &caps[0];

    // For kind:1 (non-discovery): Nip65, Hint, UserConfigured, AppRelayFallback.
    // No Indexer since kind:1 is not a discovery kind.
    let lanes: Vec<RoutingLane> = attempts.iter().map(|a| a.lane).collect();
    let nip65_pos = lanes.iter().position(|l| *l == RoutingLane::Nip65);
    let hint_pos = lanes.iter().position(|l| *l == RoutingLane::Hint);
    let uc_pos = lanes.iter().position(|l| *l == RoutingLane::UserConfigured);
    let fallback_pos = lanes
        .iter()
        .position(|l| *l == RoutingLane::AppRelayFallback);

    assert!(nip65_pos.is_some(), "Nip65 attempt missing; got {lanes:?}");
    assert!(hint_pos.is_some(), "Hint attempt missing; got {lanes:?}");
    assert!(
        uc_pos.is_some(),
        "UserConfigured attempt missing; got {lanes:?}"
    );
    assert!(
        fallback_pos.is_some(),
        "AppRelayFallback attempt missing; got {lanes:?}"
    );

    // Indexer must NOT appear for non-discovery kind.
    assert!(
        !lanes.contains(&RoutingLane::Indexer),
        "Indexer must not appear for kind:1; got {lanes:?}"
    );

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

/// Lane 4 (UserConfigured) is NOT emitted when `active_account` is absent.
/// This verifies the "only applicable lanes emit" contract.
#[test]
fn publish_no_active_account_means_no_user_configured_attempt() {
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let app = vec!["wss://app.example".to_string()];
    let c = ctx_app_only(&cache, &blocked, &app); // active_account: None

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router.route_publish(&unsigned_evt(), &c).unwrap();

    let caps = obs.publish_attempts.lock().unwrap();
    let lanes: Vec<RoutingLane> = caps[0].iter().map(|a| a.lane).collect();
    assert!(
        !lanes.contains(&RoutingLane::UserConfigured),
        "UserConfigured must not appear when active_account is None; got {lanes:?}"
    );
}

/// Publish: hint lane reports Matched even when the hint relay is also in the
/// NIP-65 write set (stacking scenario). This validates the admissible-count
/// fix for the blocker identified in review: net-new-key counting would have
/// reported Empty here.
#[test]
fn publish_hint_lane_reports_matched_even_when_relay_already_in_nip65() {
    // Seed NIP-65 write set with the same URL that will also appear as a hint.
    let cache = Arc::new(InMemoryMailboxCache::new());
    cache.fixture_upsert(
        pubkey(),
        ParsedRelayList {
            write: vec!["wss://shared.example".into()],
            ..ParsedRelayList::default()
        },
    );
    let blocked = BlockedRelaySet::new();
    let c = ctx_nip65_only(&*cache, &blocked);

    // Event with an e-tag hint pointing at the same relay.
    let evt = UnsignedEvent {
        tags: vec![vec![
            "e".into(),
            "evt-id".into(),
            "wss://shared.example".into(),
        ]],
        ..unsigned_evt()
    };

    let obs = Arc::new(AttemptCapture::default());
    let router = GenericOutboxRouter::new()
        .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
    router.route_publish(&evt, &c).unwrap();

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
        "Nip65 lane must be Matched; got {nip65:?}"
    );

    // Lane 2 (Hint) must ALSO be Matched — even though the URL was already
    // in the set from lane 1. The admissible-count fix ensures this.
    let hint = attempts.iter().find(|a| a.lane == RoutingLane::Hint);
    assert!(
        matches!(
            hint,
            Some(RouteAttempt {
                outcome: LaneOutcome::Matched { .. },
                ..
            })
        ),
        "Hint lane must be Matched even when relay already in NIP-65 set; got {hint:?}"
    );

    // AppRelayFallback must NOT appear.
    assert!(
        attempts
            .iter()
            .all(|a| a.lane != RoutingLane::AppRelayFallback),
        "AppRelayFallback must not appear; lanes resolved"
    );
}
