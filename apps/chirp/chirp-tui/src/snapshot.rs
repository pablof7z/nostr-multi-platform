use serde_json::Value;

use crate::bridge::UpdatePayload;

pub use nmp_app_chirp::{
    ActionResult, ActionStageRow, InterestRow, RelayRow, RelayWireSubRow, RuntimeMetrics,
};

#[derive(Debug, Clone, Default)]
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
            metrics: runtime_metrics_from(snapshot.get("metrics")),
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
/// the typed OP-feed `nmp.feed.home` sidecar (ADR-0038, descriptor `NOFS`) when
/// present.
///
/// During the compatibility window the host still renders from the generic
/// `Value`-based code path. When the typed `NOFS` sidecar decodes successfully we
/// re-serialize the decoded [`nmp_nip01::OpFeedSnapshot`] back into the generic
/// projection slot. Both sources carry the same type (`RootFeedSnapshot` /
/// `OpFeedSnapshot`) with the same serde derives, so every *structured* field of
/// the rendered `TimelineRow` is identical. The two `Value`s are NOT byte-shape
/// identical, though: the generic projection rides through `nmp-core`'s snapshot
/// codec, whose `encode_value` sorts object keys alphabetically, while this
/// re-serialized typed value stays in struct-field order (`preserve_order`). The
/// only render field that echoes raw key order is `TimelineRow::raw_card`, which
/// is normalized to a canonical key order at construction (see
/// `timeline::canonical_pretty`) so the rendered rows are identical regardless of
/// transport. This proves the typed decode is lossless without a render refactor.
///
/// When no typed payload is present (a pre-sidecar frame, or an unrecognized
/// descriptor such as the retired NFTS schema — ADR-0037 Commitment 4), the
/// generic `Value` projection is used verbatim, preserving the fallback.
fn decode_flatbuffer_snapshot_value(bytes: &[u8]) -> Option<Value> {
    let (mut value, typed_projections) = nmp_core::decode_snapshot_with_typed(bytes).ok()?;
    if let Some(typed_home_feed) = typed_home_feed_from_projections(&typed_projections) {
        if let Ok(typed_value) = serde_json::to_value(&typed_home_feed) {
            merge_home_feed_projection(&mut value, typed_value);
        }
    }
    Some(value)
}

/// Locate the typed OP-feed `nmp.feed.home` sidecar entry and decode it into an
/// owned [`nmp_nip01::OpFeedSnapshot`].
///
/// Returns `None` when the projection is absent or the schema id does not match
/// the NIP-01 OP-feed schema (`nmp.nip01.opfeed`) — either case falls back to
/// the generic `Value` (ADR-0037 Commitment 4). The prior NFTS descriptor
/// (`nmp.nip01.timeline`) is no longer preferred here; an `NFTS`-tagged entry is
/// treated as unrecognized and falls through to the generic projection.
fn typed_home_feed_from_projections(
    projections: &[nmp_core::TypedProjectionData],
) -> Option<nmp_nip01::OpFeedSnapshot> {
    let proj = projections
        .iter()
        .find(|p| p.key == "nmp.feed.home" && p.schema_id == nmp_nip01::OP_FEED_SCHEMA_ID)?;
    nmp_nip01::decode_op_feed_snapshot(&proj.payload).ok()
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

fn runtime_metrics_from(metrics: Option<&Value>) -> RuntimeMetrics {
    let Some(metrics) = metrics else {
        return RuntimeMetrics::default();
    };
    RuntimeMetrics {
        events_rx: number_field(metrics, "events_rx"),
        visible_items: number_field(metrics, "visible_items"),
        actor_queue_depth: number_field(metrics, "actor_queue_depth"),
        update_sequence: number_field(metrics, "update_sequence"),
    }
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
mod tests;
