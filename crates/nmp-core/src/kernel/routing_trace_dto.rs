//! V-51 phase 2 / V-75 — JSON DTO for the routing-trace projection.
//!
//! The substrate types ([`crate::substrate::RoutingSource`],
//! [`crate::substrate::PublishTrace`], etc.) deliberately do NOT carry
//! `serde::Serialize` derives — they are the producer-side router contract
//! and widening them to a wire-shape would couple every router
//! implementation to a JSON encoding it does not own.
//!
//! Instead, this module ships a thin **consumer-side** rendering helper:
//! [`projection_to_json`] walks a [`RoutingTraceProjection`] snapshot and
//! returns a [`serde_json::Value`] in a stable, Swift/wasm-friendly shape.
//!
//! The shape is:
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "capacity": 64,
//!   "publishes": [
//!     {
//!       "at_ms": 1737000000000,
//!       "kind": 1,
//!       "author": "<hex pubkey>",
//!       "event_id_short": "abcdef012345",
//!       "explicit_targets_set": false,
//!       "lane_attempts": [
//!         { "lane": "Nip65",   "outcome": { "kind": "Empty" } },
//!         { "lane": "Hint",    "outcome": { "kind": "Empty" } },
//!         { "lane": "AppRelayFallback", "outcome": { "kind": "Matched", "count": 1 } }
//!       ],
//!       "urls": [
//!         {
//!           "url": "wss://relay.example/",
//!           "lanes": [ { "kind": "Nip65", "direction": "Write" } ]
//!         }
//!       ]
//!     }
//!   ],
//!   "subscriptions": [
//!     {
//!       "at_ms": 1737000000000,
//!       "interest_id": 7,
//!       "kinds": [1, 6, 7],
//!       "authors_count": 5,
//!       "explicit_targets_set": false,
//!       "lane_attempts": [
//!         { "lane": "Nip65", "outcome": { "kind": "Matched", "count": 3 } }
//!       ],
//!       "urls": [
//!         {
//!           "url": "wss://relay.example/",
//!           "lanes": [ { "kind": "Nip65", "direction": "Read" } ]
//!         }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! `kind`-tagged lane objects match the existing pretty-printer's grammar
//! (`Nip65/Write`, `ClassRouted/Other(explicit)/Explicit`, etc.) — the
//! routing-trace integration test
//! (`crates/nmp-testing/tests/routing_trace_real_nostr.rs`, `#[ignore]`'d)
//! already pins that grammar; the JSON serialisation re-uses the same labels
//! so the Swift / TypeScript decoders agree with the human-readable form.
//!
//! `lane_attempts` is the V-75 extension: one entry per lane that ran in
//! the generic algorithm. Empty array when `explicit_targets_set` is true.
//! Lane names match [`crate::substrate::RoutingLane`] variant names;
//! `AppRelayFallback` is the sentinel for "all prior lanes were empty and
//! Lane 7 fired".
//!
//! ## Doctrine
//!
//! - **D0** — no app nouns; the DTO speaks lane attribution only.
//! - **D5** — capacity is surfaced so a host UI can render "ring N/64 full".
//! - **D6** — every step is total: a malformed lane (impossible by
//!   construction, but defended anyway) collapses to a `"kind":"Unknown"`
//!   object rather than panicking across the wire.
//! - **D8** — runs only when a host pulls the snapshot; the projection's
//!   own producer path stays zero-alloc (gated on `Option::is_some`).

use serde_json::{json, Value};

use crate::kernel::routing_trace::{
    PublishTraceEntry, RoutingTraceProjection, SubscriptionTraceEntry,
};
use crate::substrate::{
    AppRelayMode, ClassRoutingPath, Direction, EventClass, LaneOutcome, RouteAttempt, RoutingLane,
    RoutingRelayUrl, RoutingSource, UserConfiguredCategory,
};
use std::collections::BTreeSet;

/// Stable schema version for the routing-trace DTO. Bump when the shape
/// changes incompatibly so the Swift decoder can refuse unknown versions.
pub const ROUTING_TRACE_SCHEMA_VERSION: u32 = 1;

/// Render a [`RoutingTraceProjection`] into a JSON value with the stable
/// shape documented at the module level. The two ring buffers are
/// snapshot independently and rendered oldest-first (matches
/// [`RoutingTraceProjection::snapshot_publishes`] /
/// `snapshot_subscriptions`).
#[must_use]
pub fn projection_to_json(projection: &RoutingTraceProjection) -> Value {
    let publishes: Vec<Value> = projection
        .snapshot_publishes()
        .iter()
        .map(publish_entry_to_json)
        .collect();
    let subscriptions: Vec<Value> = projection
        .snapshot_subscriptions()
        .iter()
        .map(subscription_entry_to_json)
        .collect();

    json!({
        "schema_version": ROUTING_TRACE_SCHEMA_VERSION,
        "capacity": projection.capacity(),
        "publishes": publishes,
        "subscriptions": subscriptions,
    })
}

fn publish_entry_to_json(entry: &PublishTraceEntry) -> Value {
    json!({
        "at_ms": entry.at_ms,
        "kind": entry.trace.kind,
        "author": entry.trace.author,
        "event_id_short": entry.trace.event_id_short,
        "explicit_targets_set": entry.trace.explicit_targets_set,
        "lane_attempts": attempts_to_json(&entry.trace.attempts),
        "urls": urls_to_json(&entry.urls),
    })
}

fn subscription_entry_to_json(entry: &SubscriptionTraceEntry) -> Value {
    json!({
        "at_ms": entry.at_ms,
        "interest_id": entry.trace.interest_id,
        "kinds": entry.trace.kinds,
        "authors_count": entry.trace.authors_count,
        "explicit_targets_set": entry.trace.explicit_targets_set,
        "lane_attempts": attempts_to_json(&entry.trace.attempts),
        "urls": urls_to_json(&entry.urls),
    })
}

fn urls_to_json(urls: &[(RoutingRelayUrl, BTreeSet<RoutingSource>)]) -> Value {
    Value::Array(
        urls.iter()
            .map(|(url, sources)| {
                json!({
                    "url": url,
                    "lanes": sources.iter().map(lane_to_json).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// Render the per-lane attempt list (V-75) as a JSON array of objects.
/// Each entry has `"lane"` (string discriminant) and `"outcome"` (`"Matched"`
/// with a `count`, or `"Empty"`). Empty slice renders as an empty JSON array.
fn attempts_to_json(attempts: &[RouteAttempt]) -> Value {
    Value::Array(attempts.iter().map(attempt_to_json).collect())
}

fn attempt_to_json(a: &RouteAttempt) -> Value {
    let lane = routing_lane_str(a.lane);
    let outcome = match a.outcome {
        LaneOutcome::Matched { count } => json!({ "kind": "Matched", "count": count }),
        LaneOutcome::Empty => json!({ "kind": "Empty" }),
    };
    json!({ "lane": lane, "outcome": outcome })
}

fn routing_lane_str(lane: RoutingLane) -> &'static str {
    match lane {
        RoutingLane::Nip65 => "Nip65",
        RoutingLane::Hint => "Hint",
        RoutingLane::Provenance => "Provenance",
        RoutingLane::UserConfigured => "UserConfigured",
        RoutingLane::Indexer => "Indexer",
        RoutingLane::AppRelayFallback => "AppRelayFallback",
    }
}

/// Render a single [`RoutingSource`] lane as a `{ "kind": "...", ...}` object.
/// The string discriminants match the lane-attribution grammar pinned by the
/// routing-trace integration test
/// (`crates/nmp-testing/tests/routing_trace_real_nostr.rs`) so the JSON and the
/// human-readable form agree.
fn lane_to_json(source: &RoutingSource) -> Value {
    match source {
        RoutingSource::Nip65 { direction } => json!({
            "kind": "Nip65",
            "direction": direction_str(*direction),
        }),
        RoutingSource::Hint => json!({ "kind": "Hint" }),
        RoutingSource::Provenance => json!({ "kind": "Provenance" }),
        RoutingSource::UserConfigured(category) => json!({
            "kind": "UserConfigured",
            "category": user_configured_category_str(*category),
        }),
        RoutingSource::ClassRouted { class, via } => json!({
            "kind": "ClassRouted",
            "class": event_class_to_json(class),
            "via": class_routing_path_str(*via),
        }),
        RoutingSource::Indexer => json!({ "kind": "Indexer" }),
        RoutingSource::AppRelay { mode } => json!({
            "kind": "AppRelay",
            "mode": app_relay_mode_str(*mode),
        }),
    }
}

fn direction_str(d: Direction) -> &'static str {
    match d {
        Direction::Read => "Read",
        Direction::Write => "Write",
    }
}

fn user_configured_category_str(c: UserConfiguredCategory) -> &'static str {
    match c {
        UserConfiguredCategory::ActiveAccountRead => "ActiveAccountRead",
        UserConfiguredCategory::ActiveAccountWrite => "ActiveAccountWrite",
        UserConfiguredCategory::Debug => "Debug",
    }
}

fn class_routing_path_str(p: ClassRoutingPath) -> &'static str {
    match p {
        ClassRoutingPath::Explicit => "Explicit",
        ClassRoutingPath::Nip51 => "Nip51",
    }
}

fn app_relay_mode_str(m: AppRelayMode) -> &'static str {
    match m {
        AppRelayMode::Fallback => "Fallback",
        AppRelayMode::Always => "Always",
    }
}

fn event_class_to_json(c: &EventClass) -> Value {
    match c {
        EventClass::Search => json!({ "kind": "Search" }),
        EventClass::Draft => json!({ "kind": "Draft" }),
        EventClass::Wiki => json!({ "kind": "Wiki" }),
        EventClass::Other(name) => json!({ "kind": "Other", "name": name }),
    }
}

#[cfg(test)]
mod tests {
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
                explicit_targets_set: false,
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
        assert_eq!(e["explicit_targets_set"], false);
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
                explicit_targets_set: true,
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
        assert_eq!(e["explicit_targets_set"], true);
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
                explicit_targets_set: true,
                attempts: vec![],
            },
            &make_routed(
                "wss://r.example/",
                Src::ClassRouted {
                    class: EventClass::Other("explicit".into()),
                    via: ClassRoutingPath::Explicit,
                },
            ),
        );
        let v = projection_to_json(&p);
        let lane = &v["publishes"][0]["urls"][0]["lanes"][0];
        assert_eq!(lane["kind"], "ClassRouted");
        assert_eq!(lane["class"]["kind"], "Other");
        assert_eq!(lane["class"]["name"], "explicit");
        assert_eq!(lane["via"], "Explicit");
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
                    class: EventClass::Search,
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
                explicit_targets_set: false,
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
                explicit_targets_set: false,
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
            &make_routed("wss://app.example/", Src::AppRelay { mode: AppRelayMode::Fallback }),
        );
        p.on_subscription(
            SubscriptionTrace {
                interest_id: 7,
                kinds: vec![1],
                authors_count: 1,
                explicit_targets_set: false,
                attempts: vec![
                    RouteAttempt {
                        lane: RoutingLane::Nip65,
                        outcome: LaneOutcome::Matched { count: 3 },
                    },
                ],
            },
            &make_routed("wss://r.example/", Src::Nip65 { direction: Direction::Read }),
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
            let a = RouteAttempt { lane, outcome: LaneOutcome::Empty };
            let v = attempt_to_json(&a);
            assert_eq!(
                v["lane"].as_str().unwrap(),
                expected,
                "RoutingLane::{lane:?} serialized to wrong discriminant"
            );
        }
    }
}
