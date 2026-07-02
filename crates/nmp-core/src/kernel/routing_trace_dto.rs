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
//! (`Nip65/Write`, `ClassRouted/<class>/Nip51`, etc.) — the
//! routing-trace integration test
//! (`crates/nmp-testing/tests/routing_trace_real_nostr.rs`, `#[ignore]`'d)
//! already pins that grammar; the JSON serialisation re-uses the same labels
//! so the Swift / TypeScript decoders agree with the human-readable form.
//!
//! `lane_attempts` is the V-75 extension: one entry per lane that ran in
//! the generic algorithm. Lane names match [`crate::substrate::RoutingLane`] variant names;
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
        EventClass::Other(name) => json!({ "kind": "Other", "name": name }),
    }
}

#[cfg(test)]
mod tests;
