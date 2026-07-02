//! Serialization-conformance test suite for the routing-trace JSON DTO.
//!
//! Extracted from `routing_trace_dto.rs` to keep that file within the 500 LOC
//! hard cap (AGENTS.md). Pins the stable `kind`/`lane`/`outcome` discriminants
//! and the round-trippable shape produced by [`super::projection_to_json`].

use super::*;
use crate::substrate::{
    PublishTrace, RoutedRelaySet, RoutingSource as Src, RoutingTraceObserver, SubscriptionTrace,
};

fn make_routed(url: &str, source: Src) -> RoutedRelaySet {
    let mut r = RoutedRelaySet::new();
    r.add(url.into(), source);
    r
}

#[test]
fn empty_projection_renders_zero_length_arrays_and_capacity() {
    let p = RoutingTraceProjection::new();
    let v = projection_to_json(&p);
    assert_eq!(v["schema_version"], ROUTING_TRACE_SCHEMA_VERSION);
    assert_eq!(v["capacity"], 64);
    assert_eq!(v["publishes"].as_array().unwrap().len(), 0);
    assert_eq!(v["subscriptions"].as_array().unwrap().len(), 0);
}

#[test]
fn publish_entry_serializes_kind_author_and_lane() {
    let p = RoutingTraceProjection::new();
    p.on_publish(
        PublishTrace {
            kind: 1,
            author: "alice".into(),
            event_id_short: Some("abcdef012345".into()),
            attempts: vec![],
        },
        &make_routed(
            "wss://r.example/",
            Src::Nip65 {
                direction: Direction::Write,
            },
        ),
    );
    let v = projection_to_json(&p);
    let pubs = v["publishes"].as_array().unwrap();
    assert_eq!(pubs.len(), 1);
    let e = &pubs[0];
    assert_eq!(e["kind"], 1);
    assert_eq!(e["author"], "alice");
    assert_eq!(e["event_id_short"], "abcdef012345");
    let url = &e["urls"][0];
    assert_eq!(url["url"], "wss://r.example/");
    assert_eq!(url["lanes"][0]["kind"], "Nip65");
    assert_eq!(url["lanes"][0]["direction"], "Write");
}

#[test]
fn subscription_entry_serializes_interest_kinds_and_lane() {
    let p = RoutingTraceProjection::new();
    p.on_subscription(
        SubscriptionTrace {
            interest_id: 42,
            kinds: vec![1, 6, 7],
            authors_count: 3,
            attempts: vec![],
        },
        &make_routed("wss://r.example/", Src::Indexer),
    );
    let v = projection_to_json(&p);
    let subs = v["subscriptions"].as_array().unwrap();
    assert_eq!(subs.len(), 1);
    let e = &subs[0];
    assert_eq!(e["interest_id"], 42);
    assert_eq!(e["kinds"], json!([1, 6, 7]));
    assert_eq!(e["authors_count"], 3);
    assert_eq!(e["urls"][0]["lanes"][0]["kind"], "Indexer");
}

#[test]
fn class_routed_lane_carries_class_and_via() {
    let p = RoutingTraceProjection::new();
    p.on_publish(
        PublishTrace {
            kind: 30023,
            author: "alice".into(),
            event_id_short: None,
            attempts: vec![],
        },
        &make_routed(
            "wss://r.example/",
            Src::ClassRouted {
                class: EventClass::Other("nip54.wiki".into()),
                via: ClassRoutingPath::Nip51,
            },
        ),
    );
    let v = projection_to_json(&p);
    let lane = &v["publishes"][0]["urls"][0]["lanes"][0];
    assert_eq!(lane["kind"], "ClassRouted");
    assert_eq!(
        lane["class"],
        json!({ "kind": "Other", "name": "nip54.wiki" })
    );
    assert_eq!(lane["via"], "Nip51");
}

#[test]
fn all_lane_kinds_serialize_with_stable_discriminator() {
    // Doctrine guard: the seven `RoutingSource` variants each produce
    // a `kind` discriminant matching the lane-attribution grammar.
    // The routing-trace integration test
    // (`crates/nmp-testing/tests/routing_trace_real_nostr.rs`) pins that
    // grammar; the JSON form keeps the same labels so the two surfaces
    // never drift.
    let cases = vec![
        (
            Src::Nip65 {
                direction: Direction::Read,
            },
            "Nip65",
        ),
        (Src::Hint, "Hint"),
        (Src::Provenance, "Provenance"),
        (
            Src::UserConfigured(UserConfiguredCategory::Debug),
            "UserConfigured",
        ),
        (
            Src::ClassRouted {
                class: EventClass::Other("example.class".into()),
                via: ClassRoutingPath::Nip51,
            },
            "ClassRouted",
        ),
        (Src::Indexer, "Indexer"),
        (
            Src::AppRelay {
                mode: AppRelayMode::Always,
            },
            "AppRelay",
        ),
    ];
    for (src, expected_kind) in cases {
        let v = lane_to_json(&src);
        assert_eq!(
            v["kind"].as_str().unwrap(),
            expected_kind,
            "lane {src:?} serialized to wrong kind"
        );
    }
}

#[test]
fn render_json_is_round_trippable_through_serde() {
    // The DTO MUST encode to a stable string and decode back to the
    // same value — a host that round-trips through `JSON.parse`/
    // `JSONDecoder` sees no field drop or type widening.
    let p = RoutingTraceProjection::new();
    p.on_publish(
        PublishTrace {
            kind: 7,
            author: "bob".into(),
            event_id_short: Some("00aabbccddee".into()),
            attempts: vec![],
        },
        &make_routed(
            "wss://r.example/",
            Src::AppRelay {
                mode: AppRelayMode::Fallback,
            },
        ),
    );
    let v = projection_to_json(&p);
    let s = serde_json::to_string(&v).unwrap();
    let v2: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn lane_attempts_serializes_matched_and_empty_with_correct_discriminants() {
    // V-75 DTO guard: `lane_attempts` in both publish and subscription
    // entries must carry correct `"lane"` discriminants and `"outcome"`
    // shapes for `Matched { count }` and `Empty`.
    use crate::substrate::{LaneOutcome, RouteAttempt, RoutingLane};

    let p = RoutingTraceProjection::new();
    p.on_publish(
        PublishTrace {
            kind: 1,
            author: "alice".into(),
            event_id_short: None,
            attempts: vec![
                RouteAttempt {
                    lane: RoutingLane::Nip65,
                    outcome: LaneOutcome::Empty,
                },
                RouteAttempt {
                    lane: RoutingLane::Hint,
                    outcome: LaneOutcome::Empty,
                },
                RouteAttempt {
                    lane: RoutingLane::AppRelayFallback,
                    outcome: LaneOutcome::Matched { count: 2 },
                },
            ],
        },
        &make_routed(
            "wss://app.example/",
            Src::AppRelay {
                mode: AppRelayMode::Fallback,
            },
        ),
    );
    p.on_subscription(
        SubscriptionTrace {
            interest_id: 7,
            kinds: vec![1],
            authors_count: 1,
            attempts: vec![RouteAttempt {
                lane: RoutingLane::Nip65,
                outcome: LaneOutcome::Matched { count: 3 },
            }],
        },
        &make_routed(
            "wss://r.example/",
            Src::Nip65 {
                direction: Direction::Read,
            },
        ),
    );

    let v = projection_to_json(&p);

    // Publish entry: 3 attempts — two Empty then one Matched.
    let pub_attempts = &v["publishes"][0]["lane_attempts"];
    assert_eq!(pub_attempts.as_array().unwrap().len(), 3);

    let a0 = &pub_attempts[0];
    assert_eq!(a0["lane"], "Nip65");
    assert_eq!(a0["outcome"]["kind"], "Empty");

    let a1 = &pub_attempts[1];
    assert_eq!(a1["lane"], "Hint");
    assert_eq!(a1["outcome"]["kind"], "Empty");

    let a2 = &pub_attempts[2];
    assert_eq!(a2["lane"], "AppRelayFallback");
    assert_eq!(a2["outcome"]["kind"], "Matched");
    assert_eq!(a2["outcome"]["count"], 2);

    // Subscription entry: 1 attempt, Matched.
    let sub_attempts = &v["subscriptions"][0]["lane_attempts"];
    assert_eq!(sub_attempts.as_array().unwrap().len(), 1);
    assert_eq!(sub_attempts[0]["lane"], "Nip65");
    assert_eq!(sub_attempts[0]["outcome"]["kind"], "Matched");
    assert_eq!(sub_attempts[0]["outcome"]["count"], 3);
}

#[test]
fn all_routing_lane_variants_serialize_with_stable_discriminant() {
    // Doctrine guard: every `RoutingLane` variant produces a stable
    // `"lane"` string in the DTO. Prevents accidental rename drift.
    use crate::substrate::{LaneOutcome, RouteAttempt, RoutingLane};
    let cases = vec![
        (RoutingLane::Nip65, "Nip65"),
        (RoutingLane::Hint, "Hint"),
        (RoutingLane::Provenance, "Provenance"),
        (RoutingLane::UserConfigured, "UserConfigured"),
        (RoutingLane::Indexer, "Indexer"),
        (RoutingLane::AppRelayFallback, "AppRelayFallback"),
    ];
    for (lane, expected) in cases {
        let a = RouteAttempt {
            lane,
            outcome: LaneOutcome::Empty,
        };
        let v = attempt_to_json(&a);
        assert_eq!(
            v["lane"].as_str().unwrap(),
            expected,
            "RoutingLane::{lane:?} serialized to wrong discriminant"
        );
    }
}
