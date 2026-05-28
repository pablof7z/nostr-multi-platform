use super::*;

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

/// Builds a distinctive typed home-feed snapshot whose `metrics` carry a
/// sentinel `make_window_us` the generic projection never contains, so a
/// test can prove the typed sidecar was the source of truth.
fn typed_home_feed_snapshot() -> nmp_nip01::ModularTimelineSnapshot {
    nmp_nip01::ModularTimelineSnapshot {
        blocks: Vec::new(),
        cards: Vec::new(),
        page: None,
        metrics: Some(nmp_nip01::TimelineWindowMetrics {
            make_window_us: 4242,
        }),
    }
}

fn flatbuffer_payload(snapshot: Value, typed: &[nmp_core::TypedProjectionData]) -> UpdatePayload {
    UpdatePayload::FlatBuffers(nmp_core::encode_snapshot_with_typed(snapshot, typed))
}

/// ADR-0035: when a typed `nmp.feed.home` sidecar is present, the host must
/// render from the typed-decoded snapshot. The decode-then-re-serialize
/// round-trip is parity-by-construction (the generic projection is itself
/// `serde_json::to_value(ModularTimelineSnapshot)`), so `home_feed` must
/// equal the typed snapshot re-serialized — not the generic sentinel.
#[test]
fn prefers_typed_home_feed_sidecar_over_generic_projection() {
    let typed_snapshot = typed_home_feed_snapshot();
    let typed = vec![nmp_core::TypedProjectionData {
        key: "nmp.feed.home".to_string(),
        schema_id: nmp_nip01::typed_wire::SCHEMA_ID.to_string(),
        schema_version: 1,
        file_identifier: "NFTS".to_string(),
        payload: nmp_nip01::typed_wire::encode_modular_timeline_snapshot(&typed_snapshot),
    }];
    // The generic projection carries an unmistakable sentinel so we can
    // prove it was overridden by the typed path.
    let generic = serde_json::json!({
        "metrics": { "events_rx": 1 },
        "projections": {
            "nmp.feed.home": { "sentinel": "generic-must-not-win" }
        }
    });

    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(generic, &typed));

    let expected = serde_json::to_value(&typed_snapshot).expect("serialize typed snapshot");
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

/// Compatibility window: a pre-sidecar frame (no typed projections) must
/// fall back to the generic `nmp.feed.home` Value verbatim.
#[test]
fn falls_back_to_generic_home_feed_when_no_typed_sidecar() {
    let generic = serde_json::json!({
        "metrics": { "events_rx": 1 },
        "projections": {
            "nmp.feed.home": { "blocks": [], "cards": [], "legacy": true }
        }
    });

    let snapshot = SharedSnapshot::from_transport_payload(&flatbuffer_payload(generic, &[]));

    assert_eq!(
        snapshot.home_feed,
        Some(serde_json::json!({ "blocks": [], "cards": [], "legacy": true })),
        "absent typed sidecar must preserve the generic projection unchanged"
    );
}

/// A typed projection with a mismatched `schema_id` must not be consumed —
/// the host falls back to the generic Value rather than mis-decoding.
#[test]
fn ignores_typed_projection_with_wrong_schema_id() {
    let typed = vec![nmp_core::TypedProjectionData {
        key: "nmp.feed.home".to_string(),
        schema_id: "nmp.some.other.schema".to_string(),
        schema_version: 1,
        file_identifier: String::new(),
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
