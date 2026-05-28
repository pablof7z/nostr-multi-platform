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
                            "short_url": "relay.example",
                            "role_label": "Read/Write",
                            "role_tone": "primary",
                            "connection_label": "Open",
                            "connection_tone": "ok",
                            "auth_label": "OK",
                            "auth_tone": "ok",
                            "total_sub_count": 4,
                            "active_sub_count": 3,
                            "eosed_sub_count": 1,
                            "total_events_rx": 42,
                            "total_events_display": "42",
                            "reconnect_count": 2,
                            "bytes_rx_display": "1 KB",
                            "bytes_tx_display": "128 B",
                            "last_connected_display": "3s ago",
                            "last_event_display": "now",
                            "last_notice": "NOTICE text",
                            "last_error": null,
                            "wire_subs": [{
                                "wire_id": "sub-filter-json",
                                "short_wire_id": "sub-filt...",
                                "relay_url": "wss://relay.example",
                                "filter_summary": "{\"kinds\":[1],\"limit\":20}",
                                "state_label": "Open",
                                "state_tone": "ok",
                                "consumer_count_label": "1 consumer",
                                "events_rx_display": "12",
                                "eose_observed": true,
                                "opened_display": "5s ago",
                                "last_event_display": "now",
                                "eose_display": "1s ago",
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
    assert_eq!(snapshot.relays[0].connection_label, "Open");
    assert_eq!(snapshot.relays[0].relay_url, "wss://relay.example");
    assert_eq!(snapshot.relays[0].total_sub_count, 4);
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
                        "short_url": "relay.example",
                        "role_label": "Read",
                        "connection_label": "Open",
                        "active_sub_count": 1,
                        "total_events_display": "7",
                        "last_event_display": null,
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
    assert_eq!(snapshot.relays[0].short_url, "relay.example");
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
                "author_display": {
                    "name": "Alice",
                    "npub": "npub1alice",
                    "picture_url": "https://example.com/a.png"
                },
                "kind": 1,
                "created_at": 1_700_000_000u64,
                "content": "a thread root",
                "content_tree": { "nodes": [], "roots": [], "mode": "Plain" },
                "content_render": { "profiles": {}, "events": {} },
                "relation_counts": {
                    "replies": { "state": "known", "count": 2 },
                    "reactions": { "state": "known", "count": 0 },
                    "reposts": { "state": "known", "count": 0 },
                    "zaps": { "state": "known", "count": 0 }
                },
                "author_display_name": "Alice",
                "author_picture_url": "https://example.com/a.png",
                "content_preview": "a thread root"
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
    let typed: nmp_nip01::OpFeedSnapshot =
        serde_json::from_value(snapshot.clone()).expect("value decodes as OpFeedSnapshot");
    nmp_core::TypedProjectionData {
        key: "nmp.feed.home".to_string(),
        schema_id: nmp_nip01::OP_FEED_SCHEMA_ID.to_string(),
        schema_version: nmp_nip01::OP_FEED_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(nmp_nip01::OP_FEED_FILE_IDENTIFIER).into_owned(),
        payload: nmp_nip01::encode_op_feed_snapshot(&typed),
    }
}

fn flatbuffer_payload(snapshot: Value, typed: &[nmp_core::TypedProjectionData]) -> UpdatePayload {
    UpdatePayload::FlatBuffers(nmp_core::encode_snapshot_with_typed(snapshot, typed))
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
    // The generic projection carries an unmistakable sentinel so we can
    // prove it was overridden by the typed path.
    let generic = serde_json::json!({
        "metrics": { "events_rx": 1 },
        "projections": {
            "nmp.feed.home": { "sentinel": "generic-must-not-win" }
        }
    });

    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(generic, &typed));

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

/// Typed/generic render parity: a `NOFS` sidecar and the equivalent generic
/// `Value` `RootFeedSnapshot` MUST produce byte-identical `TimelineRow`s. This
/// is the load-bearing contract of Stage T2 — the typed decode is only the
/// render *source*; the render output may not diverge by encoding.
#[test]
fn typed_and_generic_home_feed_produce_identical_rows() {
    let source = op_feed_snapshot_value();

    // Typed path: NOFS sidecar present, generic subtree is a sentinel that must
    // be ignored.
    let typed_only = serde_json::json!({
        "metrics": { "events_rx": 1 },
        "projections": {
            "nmp.feed.home": { "sentinel": "generic-must-not-win" }
        }
    });
    let typed_snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(
        typed_only,
        &[nofs_projection(&source)],
    ));

    // Generic path: no typed sidecar, the source value rides the generic slot.
    let generic = serde_json::json!({
        "metrics": { "events_rx": 1 },
        "projections": { "nmp.feed.home": source }
    });
    let generic_snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(generic, &[]));

    let typed_rows = TimelineRow::from_snapshot(
        typed_snapshot.home_feed.as_ref().expect("typed home feed"),
    );
    let generic_rows = TimelineRow::from_snapshot(
        generic_snapshot.home_feed.as_ref().expect("generic home feed"),
    );

    assert!(!typed_rows.is_empty(), "fixture must yield at least one row");
    assert_eq!(
        typed_rows, generic_rows,
        "typed NOFS decode must render identical rows to the generic Value path"
    );
}

/// Compatibility window: a pre-sidecar frame (no typed projections) must
/// fall back to the generic `nmp.feed.home` Value verbatim.
#[test]
fn falls_back_to_generic_home_feed_when_no_typed_sidecar() {
    let generic = serde_json::json!({
        "metrics": { "events_rx": 1 },
        "projections": {
            "nmp.feed.home": { "cards": [], "legacy": true }
        }
    });

    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(generic, &[]));

    assert_eq!(
        snapshot.home_feed,
        Some(serde_json::json!({ "cards": [], "legacy": true })),
        "absent typed sidecar must preserve the generic projection unchanged"
    );
}

/// A typed projection with a mismatched `schema_id` (e.g. the retired NFTS
/// descriptor, or any non-`NOFS` schema) must not be consumed — the host falls
/// back to the generic Value rather than mis-decoding (ADR-0037 Commitment 4).
#[test]
fn ignores_typed_projection_with_wrong_schema_id() {
    let typed = vec![nmp_core::TypedProjectionData {
        key: "nmp.feed.home".to_string(),
        schema_id: "nmp.nip01.timeline".to_string(),
        schema_version: 1,
        file_identifier: "NFTS".to_string(),
        payload: vec![0x00, 0x01, 0x02],
    }];
    let generic = serde_json::json!({
        "metrics": { "events_rx": 1 },
        "projections": {
            "nmp.feed.home": { "kept": "generic" }
        }
    });

    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(generic, &typed));

    assert_eq!(
        snapshot.home_feed,
        Some(serde_json::json!({ "kept": "generic" })),
        "schema-id mismatch must not override the generic projection"
    );
}
