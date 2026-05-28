use serde_json::Value;

use crate::bridge::UpdatePayload;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedSnapshot {
    pub metrics: RuntimeMetrics,
    pub relays: Vec<RelayRow>,
    pub interests: Vec<InterestRow>,
    pub action_results: Vec<ActionResult>,
    pub action_stages: Vec<ActionStageRow>,
    pub home_feed: Option<Value>,
}

impl SharedSnapshot {
    #[must_use]
    pub fn from_transport_payload(payload: &UpdatePayload) -> Self {
        value_from_transport_payload(payload)
            .as_ref()
            .map(Self::from_value)
            .unwrap_or_default()
    }

    #[must_use]
    pub fn from_json_fixture(payload: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return Self::default();
        };
        // JSON fixtures may be wrapped as `{"t":"snapshot","v":<snapshot>}`.
        // Runtime transport uses FlatBuffers and enters through
        // `from_transport_payload`.
        let root = value.get("v").unwrap_or(&value);
        Self::from_value(root)
    }

    fn from_value(value: &Value) -> Self {
        let snapshot = value.get("v").unwrap_or(value);
        let projections = snapshot.get("projections");
        Self {
            metrics: RuntimeMetrics::from_value(snapshot.get("metrics")),
            relays: relays_from(projections),
            interests: interests_from(projections),
            action_results: action_results_from(projections),
            action_stages: action_stages_from(projections),
            home_feed: projections.and_then(|p| p.get("nmp.feed.home")).cloned(),
        }
    }
}

pub(crate) fn value_from_transport_payload(payload: &UpdatePayload) -> Option<Value> {
    match payload {
        UpdatePayload::FlatBuffers(bytes) => decode_flatbuffer_snapshot_value(bytes),
        UpdatePayload::JsonFixture(json) => serde_json::from_str::<Value>(json)
            .ok()
            .map(|value| value.get("v").cloned().unwrap_or(value)),
    }
}

/// Decode a FlatBuffers snapshot frame into the generic `Value` tree, preferring
/// the typed `nmp.feed.home` sidecar (ADR-0035) when present.
///
/// During the compatibility window the host still renders from the generic
/// `Value`-based code path. When the typed NFTS sidecar decodes successfully we
/// re-serialize the [`nmp_nip01::ModularTimelineSnapshot`] back into the generic
/// projection slot. Because the generic `nmp.feed.home` projection is itself
/// produced by `serde_json::to_value(ModularTimelineSnapshot)`, this round-trip
/// is parity-by-construction: same type, same serde derives, identical `Value`
/// shape. It proves the typed decode is lossless without a render refactor.
///
/// When no typed payload is present (a pre-sidecar frame), the generic `Value`
/// projection is used verbatim, preserving the compatibility fallback.
fn decode_flatbuffer_snapshot_value(bytes: &[u8]) -> Option<Value> {
    let (mut value, typed_projections) = nmp_core::decode_snapshot_with_typed(bytes).ok()?;
    if let Some(typed_home_feed) = typed_home_feed_from_projections(&typed_projections) {
        if let Ok(typed_value) = serde_json::to_value(&typed_home_feed) {
            merge_home_feed_projection(&mut value, typed_value);
        }
    }
    Some(value)
}

/// Locate the typed `nmp.feed.home` sidecar entry and decode it into an owned
/// [`nmp_nip01::ModularTimelineSnapshot`].
///
/// Returns `None` when the projection is absent or the schema id does not match
/// the NIP-01 timeline schema — either case falls back to the generic `Value`.
fn typed_home_feed_from_projections(
    projections: &[nmp_core::TypedProjectionData],
) -> Option<nmp_nip01::ModularTimelineSnapshot> {
    let proj = projections.iter().find(|p| {
        p.key == "nmp.feed.home" && p.schema_id == nmp_nip01::typed_wire::SCHEMA_ID
    })?;
    nmp_nip01::typed_wire::decode_modular_timeline_snapshot(&proj.payload).ok()
}

/// Overwrite `value["projections"]["nmp.feed.home"]` with the typed-derived
/// snapshot value. No-op if the snapshot has no `projections` object.
///
/// Mirrors [`SharedSnapshot::from_value`], which reaches through an optional
/// `"v"` envelope before reading `projections`, so the typed value lands in the
/// same slot the render path reads from.
fn merge_home_feed_projection(value: &mut Value, typed_home_feed: Value) {
    let snapshot = match value.get_mut("v") {
        Some(inner) => inner,
        None => value,
    };
    if let Some(Value::Object(projections)) = snapshot.get_mut("projections") {
        projections.insert("nmp.feed.home".to_string(), typed_home_feed);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeMetrics {
    pub events_rx: u64,
    pub visible_items: u64,
    pub actor_queue_depth: u64,
    pub update_sequence: u64,
}

impl RuntimeMetrics {
    fn from_value(metrics: Option<&Value>) -> Self {
        let Some(metrics) = metrics else {
            return Self::default();
        };
        Self {
            events_rx: number_field(metrics, "events_rx"),
            visible_items: number_field(metrics, "visible_items"),
            actor_queue_depth: number_field(metrics, "actor_queue_depth"),
            update_sequence: number_field(metrics, "update_sequence"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelayRow {
    pub relay_url: String,
    pub short_url: String,
    pub role_label: String,
    pub role_tone: String,
    pub connection_label: String,
    pub connection_tone: String,
    pub auth_label: String,
    pub auth_tone: String,
    pub total_sub_count: u64,
    pub active_sub_count: u64,
    pub eosed_sub_count: u64,
    pub total_events_rx: u64,
    pub total_events_display: String,
    pub reconnect_count: u64,
    pub bytes_rx_display: Option<String>,
    pub bytes_tx_display: Option<String>,
    pub last_connected_display: Option<String>,
    pub last_event_display: Option<String>,
    pub last_notice: Option<String>,
    pub last_error: Option<String>,
    pub wire_subs: Vec<RelayWireSubRow>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelayWireSubRow {
    pub wire_id: String,
    pub short_wire_id: String,
    pub relay_url: String,
    pub filter_summary: String,
    pub state_label: String,
    pub state_tone: String,
    pub consumer_count_label: String,
    pub events_rx_display: Option<String>,
    pub eose_observed: bool,
    pub opened_display: String,
    pub last_event_display: Option<String>,
    pub eose_display: Option<String>,
    pub close_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterestRow {
    pub key: String,
    pub state: String,
    pub refcount: u64,
    pub cache_coverage: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResult {
    pub correlation_id: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionStageRow {
    pub correlation_id: String,
    pub stage: String,
    pub reason: Option<String>,
}

fn relays_from(projections: Option<&Value>) -> Vec<RelayRow> {
    projections
        .and_then(|p| p.get("relay_diagnostics"))
        .and_then(|diag| diag.get("relays"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| RelayRow {
            relay_url: string_field(row, "relay_url"),
            short_url: string_field(row, "short_url"),
            role_label: string_field(row, "role_label"),
            role_tone: string_field(row, "role_tone"),
            connection_label: string_field(row, "connection_label"),
            connection_tone: string_field(row, "connection_tone"),
            auth_label: string_field(row, "auth_label"),
            auth_tone: string_field(row, "auth_tone"),
            total_sub_count: number_field(row, "total_sub_count"),
            active_sub_count: number_field(row, "active_sub_count"),
            eosed_sub_count: number_field(row, "eosed_sub_count"),
            total_events_rx: number_field(row, "total_events_rx"),
            total_events_display: string_field(row, "total_events_display"),
            reconnect_count: number_field(row, "reconnect_count"),
            bytes_rx_display: optional_string(row, "bytes_rx_display"),
            bytes_tx_display: optional_string(row, "bytes_tx_display"),
            last_connected_display: optional_string(row, "last_connected_display"),
            last_event_display: optional_string(row, "last_event_display"),
            last_notice: optional_string(row, "last_notice"),
            last_error: optional_string(row, "last_error"),
            wire_subs: relay_wire_subs_from(row),
        })
        .collect()
}

fn relay_wire_subs_from(row: &Value) -> Vec<RelayWireSubRow> {
    row.get("wire_subs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|sub| RelayWireSubRow {
            wire_id: string_field(sub, "wire_id"),
            short_wire_id: string_field(sub, "short_wire_id"),
            relay_url: string_field(sub, "relay_url"),
            filter_summary: string_field(sub, "filter_summary"),
            state_label: string_field(sub, "state_label"),
            state_tone: string_field(sub, "state_tone"),
            consumer_count_label: string_field(sub, "consumer_count_label"),
            events_rx_display: optional_string(sub, "events_rx_display"),
            eose_observed: sub
                .get("eose_observed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            opened_display: string_field(sub, "opened_display"),
            last_event_display: optional_string(sub, "last_event_display"),
            eose_display: optional_string(sub, "eose_display"),
            close_reason: optional_string(sub, "close_reason"),
        })
        .collect()
}

fn interests_from(projections: Option<&Value>) -> Vec<InterestRow> {
    projections
        .and_then(|p| p.get("relay_diagnostics"))
        .and_then(|diag| diag.get("interests"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| InterestRow {
            key: string_field(row, "key"),
            state: string_field(row, "state"),
            refcount: number_field(row, "refcount"),
            cache_coverage: string_field(row, "cache_coverage"),
        })
        .collect()
}

fn action_results_from(projections: Option<&Value>) -> Vec<ActionResult> {
    projections
        .and_then(|p| p.get("action_results"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| ActionResult {
            correlation_id: string_field(row, "correlation_id"),
            status: string_field(row, "status"),
            error: optional_string(row, "error"),
        })
        .filter(|row| !row.correlation_id.is_empty())
        .collect()
}

fn action_stages_from(projections: Option<&Value>) -> Vec<ActionStageRow> {
    let mut rows = Vec::new();
    let Some(stages) = projections
        .and_then(|p| p.get("action_stages"))
        .and_then(Value::as_object)
    else {
        return rows;
    };

    for (correlation_id, entries) in stages {
        let Some(last) = entries.as_array().and_then(|items| items.last()) else {
            continue;
        };
        rows.push(ActionStageRow {
            correlation_id: correlation_id.clone(),
            stage: string_field(last, "stage"),
            reason: optional_string(last, "reason"),
        });
    }
    rows
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn number_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

#[cfg(test)]
mod tests {
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

    fn flatbuffer_payload(
        snapshot: Value,
        typed: &[nmp_core::TypedProjectionData],
    ) -> UpdatePayload {
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

        let snapshot =
            SharedSnapshot::from_transport_payload(&flatbuffer_payload(generic, &typed));

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

        let snapshot =
            SharedSnapshot::from_transport_payload(&flatbuffer_payload(generic, &typed));

        assert_eq!(
            snapshot.home_feed,
            Some(serde_json::json!({ "kept": "generic" })),
            "schema-id mismatch must not override the generic projection"
        );
    }
}
