use super::*;
use crate::timeline::TimelineRow;

#[test]
fn parses_direct_shared_diagnostics_and_action_projections() {
    let payload = sample_payload().to_string();

    let snapshot = SharedSnapshot::from_json_fixture(&payload);

    assert_sample_snapshot(snapshot);
}

#[test]
fn parses_enveloped_shared_diagnostics_and_action_projections() {
    let payload = serde_json::json!({
        "t": "FullState",
        "v": sample_payload()
    })
    .to_string();

    let snapshot = SharedSnapshot::from_json_fixture(&payload);

    assert_sample_snapshot(snapshot);
}

fn sample_payload() -> Value {
    serde_json::from_str(
        r#"{
                "metrics": {
                    "events_rx": 5,
                    "visible_items": 2,
                    "actor_queue_depth": 1,
                    "update_sequence": 9
                },
                "projections": {
                    "relay_diagnostics": {
                        "relays": [{
                            "relay_url": "wss://relay.example",
                            "role": "content",
                            "connection": "open",
                            "auth": "ok",
                            "total_sub_count": 4,
                            "active_sub_count": 3,
                            "eosed_sub_count": 1,
                            "total_events_rx": 42,
                            "reconnect_count": 2,
                            "discovery_kinds": [0, 3, 10002],
                            "bytes_rx": 1024,
                            "bytes_tx": 128,
                            "last_connected_ms": 1700000000000,
                            "last_event_ms": 1700000003000,
                            "last_notice": "NOTICE text",
                            "last_error": null,
                            "wire_subs": [{
                                "wire_id": "sub-filter-json",
                                "relay_url": "wss://relay.example",
                                "filter_summary": "{\"kinds\":[1],\"limit\":20}",
                                "state": "open",
                                "consumer_count": 1,
                                "events_rx": 12,
                                "eose_observed": true,
                                "opened_ms": 1700000000000,
                                "last_event_ms": 1700000003000,
                                "eose_ms": 1700000001000,
                                "close_reason": null
                            }]
                        }],
                        "interests": [{
                            "key": "home",
                            "state": "active",
                            "refcount": 1,
                            "cache_coverage": "live"
                        }]
                    },
                    "action_results": [{
                        "correlation_id": "corr-1",
                        "status": "published",
                        "error": null
                    }],
                    "action_stages": {
                        "corr-2": [
                            {"stage": "requested", "at_ms": 1},
                            {"stage": "publishing", "at_ms": 2}
                        ]
                    }
                }
            }"#,
    )
    .expect("valid sample payload")
}

fn assert_sample_snapshot(snapshot: SharedSnapshot) {
    assert_eq!(snapshot.metrics.events_rx, 5);
    assert_eq!(snapshot.relays[0].connection, "open");
    assert_eq!(snapshot.relays[0].relay_url, "wss://relay.example");
    assert_eq!(snapshot.relays[0].total_sub_count, 4);
    assert_eq!(snapshot.relays[0].discovery_kinds, vec![0, 3, 10002]);
    assert_eq!(snapshot.relays[0].wire_subs[0].state, "open");
    assert_eq!(snapshot.relays[0].wire_subs[0].consumer_count, 1);
    assert_eq!(snapshot.relays[0].wire_subs[0].events_rx, 12);
    assert_eq!(
        snapshot.relays[0].wire_subs[0].filter_summary,
        "{\"kinds\":[1],\"limit\":20}"
    );
    assert_eq!(snapshot.interests[0].cache_coverage, "live");
    assert_eq!(snapshot.action_results[0].correlation_id, "corr-1");
    assert_eq!(snapshot.action_stages[0].stage, "publishing");
}

/// Legacy JSON fixtures may arrive wrapped as
/// `{"t":"snapshot","v":<snapshot>}`. The parser must reach into `v` so
/// `projections`/`metrics` resolve.
#[test]
fn unwraps_snapshot_envelope_when_present() {
    let payload = serde_json::json!({
        "t": "snapshot",
        "v": {
            "metrics": {
                "events_rx": 7,
                "visible_items": 0,
                "actor_queue_depth": 0,
                "update_sequence": 3
            },
            "projections": {
                "relay_diagnostics": {
                    "relays": [{
                        "relay_url": "wss://relay.example",
                        "role": "content",
                        "connection": "open",
                        "active_sub_count": 1,
                        "total_events_rx": 7,
                        "last_event_ms": 0,
                        "last_error": null
                    }],
                    "interests": []
                },
                "action_results": [],
                "action_stages": {}
            }
        }
    })
    .to_string();

    let snapshot = SharedSnapshot::from_json_fixture(&payload);

    assert_eq!(snapshot.metrics.events_rx, 7);
    assert_eq!(snapshot.relays.len(), 1);
    assert_eq!(snapshot.relays[0].relay_url, "wss://relay.example");
    assert_eq!(snapshot.relays[0].connection, "open");
}

/// A `RootFeedSnapshot`-shaped JSON value (ADR-0038): one thread-root card
/// carrying a NIP-10 reply attribution, plus a populated paging/metrics window.
///
/// This is the SINGLE source of truth for the typed/generic parity tests: it
/// deserializes into [`nmp_nip01::OpFeedSnapshot`] (the typed payload the `NOFS`
/// encoder consumes) AND serves verbatim as the generic `Value` fallback. Using
/// one source value guarantees the two encodings are two views of the same
/// `RootFeedSnapshot`, exactly as the producer emits them during the rollout.
fn op_feed_snapshot_value() -> Value {
    serde_json::json!({
        "cards": [{
            "card": {
                "id": "aa".repeat(32),
                "author_pubkey": "bb".repeat(32),
                "kind": 1,
                "created_at": 1_700_000_000u64,
                "content": "a thread root",
                "content_tree": { "nodes": [], "roots": [], "mode": "Plain" },
                "relation_counts": {
                    "replies": { "state": "known", "count": 2 },
                    "reactions": { "state": "known", "count": 0 },
                    "reposts": { "state": "known", "count": 0 },
                    "comments": { "state": "known", "count": 0 },
                    "zaps": { "state": "known", "count": 0 }
                }
            },
            "attribution": [{
                "author_pubkey": "cc".repeat(32),
                "author_display": {
                    "name": "Bob",
                    "npub": "npub1bob",
                    "picture_url": null
                },
                "author_display_name": "Bob",
                "author_picture_url": null,
                "reply_event_id": "dd".repeat(32),
                "reply_created_at": 1_700_000_500u64
            }]
        }],
        // `next_cursor` is `skip_serializing_if = Option::is_none`, so it is
        // omitted here to match the typed decode's re-serialized shape exactly.
        "page": { "limit": 20, "has_more": false, "total_blocks": 1 },
        "metrics": { "make_window_us": 4242 }
    })
}

/// The `NOFS` typed sidecar entry for the given `RootFeedSnapshot` value.
///
/// Deserializes the value into the typed [`nmp_nip01::OpFeedSnapshot`] and
/// encodes it with the OP-feed encoder — exactly the path the producer's
/// `register_typed_snapshot_projection` closure takes (ADR-0038 Commitment 5).
fn nofs_projection(snapshot: &Value) -> nmp_core::TypedProjectionData {
    nofs_projection_for("nmp.feed.home", snapshot)
}

fn nofs_projection_for(key: &str, snapshot: &Value) -> nmp_core::TypedProjectionData {
    let typed: nmp_nip01::OpFeedSnapshot =
        serde_json::from_value(snapshot.clone()).expect("value decodes as OpFeedSnapshot");
    nmp_core::TypedProjectionData {
        key: key.to_string(),
        schema_id: nmp_nip01::OP_FEED_SCHEMA_ID.to_string(),
        schema_version: nmp_nip01::OP_FEED_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(nmp_nip01::OP_FEED_FILE_IDENTIFIER).into_owned(),
        payload: nmp_nip01::encode_op_feed_snapshot(&typed),
        ..Default::default()
    }
}

fn flatbuffer_payload(typed: &[nmp_core::TypedProjectionData]) -> UpdatePayload {
    // PR-B: frames carry the typed Tier-3 envelope + typed sidecar only — the
    // generic `payload:Value` slot no longer exists in the encoder, so "the
    // generic subtree must not win" is now a compile-time impossibility.
    UpdatePayload::FlatBuffers(nmp_core::encode_snapshot_frame(
        &nmp_core::SnapshotEnvelope::default(),
        typed,
    ))
}

/// ADR-0038 Commitment 4: when a typed `NOFS` sidecar is present for
/// `nmp.feed.home`, the host MUST prefer the typed-decoded snapshot and MUST
/// ignore the generic `Value` subtree. The decode-then-re-serialize round-trip
/// is parity-by-construction (the generic projection is itself
/// `serde_json::to_value(RootFeedSnapshot)`), so `home_feed` must equal the
/// typed snapshot re-serialized — not the generic sentinel.
#[test]
fn prefers_typed_home_feed_sidecar_over_generic_projection() {
    let source = op_feed_snapshot_value();
    let typed = vec![nofs_projection(&source)];

    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(&typed));

    // The home feed must equal the typed source re-serialized (the typed decode
    // round-trips losslessly through `OpFeedSnapshot` serde), not the generic
    // sentinel. Canonicalize the source the same way the decode path does so the
    // comparison is independent of serde field-ordering / skip details.
    let expected: nmp_nip01::OpFeedSnapshot =
        serde_json::from_value(source).expect("source decodes as OpFeedSnapshot");
    let expected = serde_json::to_value(&expected).expect("re-serialize expected");
    assert_eq!(snapshot.home_feed, Some(expected));
    assert_eq!(
        snapshot
            .home_feed
            .as_ref()
            .and_then(|f| f.get("metrics"))
            .and_then(|m| m.get("make_window_us"))
            .and_then(Value::as_u64),
        Some(4242),
        "typed metrics sentinel must survive the decode/re-serialize round-trip"
    );
}

/// Typed render round-trip: a `NOFS` sidecar present in the FlatBuffers frame
/// produces `home_feed` from the typed decode path. The generic sentinel in the
/// `payload:Value` slot is ignored (the slot is `(deprecated)` in PR-B — the
/// producer no longer emits it, and the TUI shell no longer reads it).
///
/// This replaces the former "typed/generic parity" test: after PR-B there is
/// only ONE render path (typed), so the generic comparison is removed.
#[test]
fn typed_home_feed_sidecar_produces_feed_rows() {
    let source = op_feed_snapshot_value();

    // Typed path: NOFS sidecar present (PR-B: no generic subtree exists).
    let typed_snapshot =
        SharedSnapshot::from_transport_payload(&flatbuffer_payload(&[nofs_projection(&source)]));

    let typed_rows =
        TimelineRow::from_snapshot(typed_snapshot.home_feed.as_ref().expect("typed home feed"));
    assert!(
        !typed_rows.is_empty(),
        "typed NOFS sidecar must yield at least one row"
    );
}

#[test]
fn decodes_dynamic_author_and_thread_op_feed_sidecars() {
    let home = op_feed_snapshot_value();
    let mut author = op_feed_snapshot_value();
    author["cards"][0]["card"]["id"] = Value::String("author-row".to_string());
    let mut thread = op_feed_snapshot_value();
    thread["cards"][0]["card"]["id"] = Value::String("thread-row".to_string());
    let author_key = format!("nmp.feed.author.{}", "aa".repeat(32));
    let thread_key = format!("nmp.feed.thread.{}", "bb".repeat(32));
    let typed = vec![
        nofs_projection_for("nmp.feed.home", &home),
        nofs_projection_for(&author_key, &author),
        nofs_projection_for(&thread_key, &thread),
    ];

    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(&typed));

    assert_eq!(snapshot.feeds.len(), 3);
    assert_eq!(
        snapshot
            .feeds
            .get(&author_key)
            .and_then(FeedProjection::as_value)
            .and_then(|feed| feed.get("cards"))
            .and_then(Value::as_array)
            .and_then(|cards| cards.first())
            .and_then(|entry| entry.get("card"))
            .and_then(|card| card.get("id"))
            .and_then(Value::as_str),
        Some("author-row")
    );
    assert_eq!(
        snapshot
            .feeds
            .get(&thread_key)
            .and_then(FeedProjection::as_value)
            .and_then(|feed| feed.get("cards"))
            .and_then(Value::as_array)
            .and_then(|cards| cards.first())
            .and_then(|entry| entry.get("card"))
            .and_then(|card| card.get("id"))
            .and_then(Value::as_str),
        Some("thread-row")
    );
}

#[test]
fn typed_cleared_dynamic_feed_sidecar_is_preserved() {
    let author_key = format!("nmp.feed.author.{}", "aa".repeat(32));
    let typed = vec![nmp_core::TypedProjectionData {
        key: author_key.clone(),
        state: nmp_core::WireProjectionState::Cleared,
        ..Default::default()
    }];

    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(&typed));

    assert_eq!(
        snapshot.feeds.get(&author_key),
        Some(&FeedProjection::Cleared),
        "a Cleared dynamic feed row must survive decode so AppState can tear down stale rows"
    );
}

/// PR-B: after `payload:Value` is `(deprecated)`, a FlatBuffers frame with no
/// typed `nmp.feed.home` sidecar produces `home_feed = None`. The compatibility
/// window (generic fallback) is closed.
#[test]
fn no_typed_sidecar_yields_none_home_feed() {
    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(&[]));

    assert_eq!(
        snapshot.home_feed, None,
        "PR-B: no typed sidecar → home_feed must be None (generic fallback removed)"
    );
}

/// PR-B: a typed projection with a mismatched `schema_id` (e.g. the retired
/// NFTS descriptor) is rejected, and `home_feed` is `None` — no generic
/// fallback (ADR-0037 Commitment 4, PR-B revision).
#[test]
fn ignores_typed_projection_with_wrong_schema_id() {
    let typed = vec![nmp_core::TypedProjectionData {
        key: "nmp.feed.home".to_string(),
        schema_id: "nmp.nip01.timeline".to_string(),
        schema_version: 1,
        file_identifier: "NFTS".to_string(),
        payload: vec![0x00, 0x01, 0x02],
        ..Default::default()
    }];

    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(&typed));

    assert_eq!(
        snapshot.home_feed, None,
        "PR-B: schema-id mismatch → home_feed must be None (generic fallback removed)"
    );
}
