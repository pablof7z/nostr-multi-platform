use std::collections::HashMap;

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
    pub feeds: HashMap<String, FeedProjection>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FeedProjection {
    Changed(Value),
    Cleared,
}

impl FeedProjection {
    #[must_use]
    pub fn as_value(&self) -> Option<&Value> {
        match self {
            Self::Changed(value) => Some(value),
            Self::Cleared => None,
        }
    }
}

impl SharedSnapshot {
    #[must_use]
    pub fn from_transport_payload(payload: &UpdatePayload) -> Self {
        match payload {
            UpdatePayload::FlatBuffers(bytes) => decode_flatbuffer_snapshot(bytes),
            UpdatePayload::JsonFixture(json) => {
                let Ok(value) = serde_json::from_str::<Value>(json) else {
                    return Self::default();
                };
                // JSON fixtures may be wrapped as `{"t":"snapshot","v":<snapshot>}`.
                let root = value.get("v").unwrap_or(&value);
                Self::from_value(root)
            }
        }
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
        let feeds = feeds_from(projections);
        Self {
            metrics: runtime_metrics_from(snapshot.get("metrics")),
            relays: relays_from(projections),
            interests: interests_from(projections),
            action_results: action_results_from(projections),
            action_stages: action_stages_from(projections),
            home_feed: feeds
                .get("nmp.feed.home")
                .and_then(FeedProjection::as_value)
                .cloned(),
            feeds,
        }
    }
}

/// Decode a FlatBuffers snapshot frame using typed-first paths (ADR-0044 PR-B).
///
/// All data comes from typed channels:
///
/// - Tier-3 envelope (`decode_snapshot_envelope`) supplies the runtime metrics
///   fields (`events_rx`, `visible_items`, `actor_queue_depth`,
///   `update_sequence`).
/// - The `typed_projections` sidecar supplies `relay_diagnostics`,
///   `action_results`, `action_stages`, and dynamic `nmp.feed.*` rows.
///
/// When a typed sidecar entry is absent or fails to decode (e.g. the slot was
/// not yet emitted, or a schema mismatch — ADR-0037 Commitment 4), the
/// corresponding field returns its zero value — no `payload:Value` fallback.
/// PR-B completes the typed-first migration: `FeatureSnapshot` and
/// chirp-desktop `Snapshot` also read from typed sidecars; `payload:Value`
/// emission is stopped in `encode_snapshot_with_envelope` once all read sites
/// are flipped (this PR, closes #991/#979).
fn decode_flatbuffer_snapshot(bytes: &[u8]) -> SharedSnapshot {
    // Tier-3 envelope — metrics and status fields live on SnapshotFrame directly.
    let envelope = nmp_core::decode_snapshot_envelope(bytes).unwrap_or_default();
    let metrics = RuntimeMetrics {
        events_rx: envelope.events_rx,
        visible_items: envelope.visible_items,
        actor_queue_depth: envelope.actor_queue_depth as u64,
        update_sequence: envelope.update_sequence,
    };

    // Typed sidecar — one entry per built-in projection key.
    let typed_projections = nmp_core::decode_snapshot_typed_projections(bytes).unwrap_or_default();

    let relays = typed_relay_rows(&typed_projections);
    let interests = typed_interest_rows(&typed_projections);
    let action_results = typed_action_results(&typed_projections);
    let action_stages = typed_action_stages(&typed_projections);
    let feeds = typed_op_feeds(&typed_projections);
    let home_feed = feeds
        .get("nmp.feed.home")
        .and_then(FeedProjection::as_value)
        .cloned();

    SharedSnapshot {
        metrics,
        relays,
        interests,
        action_results,
        action_stages,
        home_feed,
        feeds,
    }
}

// ---------------------------------------------------------------------------
// Typed sidecar decoders
// ---------------------------------------------------------------------------

/// Decode the `relay_diagnostics` typed sidecar and map to chirp relay rows.
///
/// Returns an empty vec if the sidecar is absent or fails to decode.
fn typed_relay_rows(projections: &[nmp_core::TypedProjectionData]) -> Vec<RelayRow> {
    let Some(entry) = projections.iter().find(|p| {
        p.key == nmp_core::typed_projections::RELAY_DIAGNOSTICS_SCHEMA_ID
            && p.schema_id == nmp_core::typed_projections::RELAY_DIAGNOSTICS_SCHEMA_ID
    }) else {
        return Vec::new();
    };
    let Ok(model) = nmp_core::typed_projections::decode_relay_diagnostics(&entry.payload) else {
        return Vec::new();
    };
    model.relays.into_iter().map(relay_row_from_typed).collect()
}

/// Decode the `relay_diagnostics` typed sidecar and map to chirp interest rows.
///
/// Returns an empty vec if the sidecar is absent or fails to decode.
fn typed_interest_rows(projections: &[nmp_core::TypedProjectionData]) -> Vec<InterestRow> {
    let Some(entry) = projections.iter().find(|p| {
        p.key == nmp_core::typed_projections::RELAY_DIAGNOSTICS_SCHEMA_ID
            && p.schema_id == nmp_core::typed_projections::RELAY_DIAGNOSTICS_SCHEMA_ID
    }) else {
        return Vec::new();
    };
    let Ok(model) = nmp_core::typed_projections::decode_relay_diagnostics(&entry.payload) else {
        return Vec::new();
    };
    model
        .interests
        .into_iter()
        .map(interest_row_from_typed)
        .collect()
}

/// Decode the `action_results` typed sidecar and map to chirp action result rows.
///
/// Returns an empty vec if the sidecar is absent or fails to decode.
fn typed_action_results(projections: &[nmp_core::TypedProjectionData]) -> Vec<ActionResult> {
    let Some(entry) = projections.iter().find(|p| {
        p.key == nmp_core::typed_projections::ACTION_RESULTS_SCHEMA_ID
            && p.schema_id == nmp_core::typed_projections::ACTION_RESULTS_SCHEMA_ID
    }) else {
        return Vec::new();
    };
    let Ok(model) = nmp_core::typed_projections::decode_action_results(&entry.payload) else {
        return Vec::new();
    };
    model
        .results
        .into_iter()
        .filter(|r| !r.correlation_id.is_empty())
        .map(action_result_from_typed)
        .collect()
}

/// Decode the `action_stages` typed sidecar and map to chirp action stage rows
/// (last-stage-per-correlation-id).
///
/// Returns an empty vec if the sidecar is absent or fails to decode.
fn typed_action_stages(projections: &[nmp_core::TypedProjectionData]) -> Vec<ActionStageRow> {
    let Some(entry) = projections.iter().find(|p| {
        p.key == nmp_core::typed_projections::ACTION_STAGES_SCHEMA_ID
            && p.schema_id == nmp_core::typed_projections::ACTION_STAGES_SCHEMA_ID
    }) else {
        return Vec::new();
    };
    let Ok(model) = nmp_core::typed_projections::decode_action_stages(&entry.payload) else {
        return Vec::new();
    };
    model
        .entries
        .into_iter()
        .filter_map(
            |(correlation_id, history): (
                String,
                Vec<nmp_core::typed_projections::ActionStageEntryRow>,
            )| {
                let last = history.into_iter().last()?;
                Some(ActionStageRow {
                    correlation_id,
                    stage: last.stage,
                    reason: last.reason,
                })
            },
        )
        .collect()
}

/// Decode every typed `nmp.feed.*` NOFS sidecar by projection key.
///
/// Absent, wrong-schema, or corrupt sidecars are ignored — preserving ADR-0037
/// Commitment 4. After PR-B the generic `payload:Value` fallback is gone.
fn typed_op_feeds(
    projections: &[nmp_core::TypedProjectionData],
) -> HashMap<String, FeedProjection> {
    projections
        .iter()
        .filter(|p| p.key.starts_with("nmp.feed."))
        .filter_map(|proj| {
            if proj.state == nmp_core::WireProjectionState::Cleared {
                return Some((proj.key.clone(), FeedProjection::Cleared));
            }
            if proj.schema_id != nmp_nip01::OP_FEED_SCHEMA_ID {
                return None;
            }
            let decoded = nmp_nip01::decode_op_feed_snapshot(&proj.payload).ok()?;
            let value = serde_json::to_value(&decoded).ok()?;
            Some((proj.key.clone(), FeedProjection::Changed(value)))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Type-mapping helpers: nmp_core typed_projections DTOs → nmp_app_chirp types
// ---------------------------------------------------------------------------

fn relay_row_from_typed(row: nmp_core::typed_projections::RelayRow) -> RelayRow {
    RelayRow {
        relay_url: row.relay_url,
        role: row.role,
        connection: row.connection,
        auth: row.auth,
        total_sub_count: row.total_sub_count as u64,
        active_sub_count: row.active_sub_count as u64,
        eosed_sub_count: row.eosed_sub_count as u64,
        total_events_rx: row.total_events_rx,
        reconnect_count: row.reconnect_count as u64,
        discovery_kinds: row.discovery_kinds,
        bytes_rx: row.bytes_rx,
        bytes_tx: row.bytes_tx,
        last_connected_ms: row.last_connected_ms,
        last_event_ms: row.last_event_ms,
        last_notice: row.last_notice,
        last_error: row.last_error,
        wire_subs: row.wire_subs.into_iter().map(wire_sub_from_typed).collect(),
    }
}

fn wire_sub_from_typed(sub: nmp_core::typed_projections::WireSubRow) -> RelayWireSubRow {
    RelayWireSubRow {
        wire_id: sub.wire_id,
        relay_url: sub.relay_url,
        filter_summary: sub.filter_summary,
        state: sub.state,
        consumer_count: sub.consumer_count,
        events_rx: sub.events_rx,
        eose_observed: sub.eose_observed,
        opened_ms: sub.opened_ms,
        last_event_ms: sub.last_event_ms,
        eose_ms: sub.eose_ms,
        close_reason: sub.close_reason,
    }
}

fn interest_row_from_typed(row: nmp_core::typed_projections::InterestRow) -> InterestRow {
    InterestRow {
        key: row.key,
        state: row.state,
        refcount: row.refcount as u64,
        cache_coverage: row.cache_coverage,
    }
}

fn action_result_from_typed(row: nmp_core::typed_projections::ActionResultRow) -> ActionResult {
    ActionResult {
        correlation_id: row.correlation_id,
        status: row.status,
        error: row.error,
    }
}

// `value_from_transport_payload` was removed in PR-B.
// `FeatureSnapshot::from_transport_payload` now decodes typed sidecars
// directly via `nmp_core::decode_snapshot_typed_projections`; the
// generic `payload:Value` tree is no longer read by any Rust shell.

// ---------------------------------------------------------------------------
// JSON fixture decode helpers (used by `from_json_fixture` only)
// ---------------------------------------------------------------------------

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
            role: string_field(row, "role"),
            connection: string_field(row, "connection"),
            auth: string_field(row, "auth"),
            total_sub_count: number_field(row, "total_sub_count"),
            active_sub_count: number_field(row, "active_sub_count"),
            eosed_sub_count: number_field(row, "eosed_sub_count"),
            total_events_rx: number_field(row, "total_events_rx"),
            reconnect_count: number_field(row, "reconnect_count"),
            discovery_kinds: u64_vec_field(row, "discovery_kinds"),
            bytes_rx: number_field(row, "bytes_rx"),
            bytes_tx: number_field(row, "bytes_tx"),
            last_connected_ms: number_field(row, "last_connected_ms"),
            last_event_ms: number_field(row, "last_event_ms"),
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
            relay_url: string_field(sub, "relay_url"),
            filter_summary: string_field(sub, "filter_summary"),
            state: string_field(sub, "state"),
            consumer_count: number_field(sub, "consumer_count") as u32,
            events_rx: number_field(sub, "events_rx"),
            eose_observed: sub
                .get("eose_observed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            opened_ms: number_field(sub, "opened_ms"),
            last_event_ms: number_field(sub, "last_event_ms"),
            eose_ms: number_field(sub, "eose_ms"),
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

fn feeds_from(projections: Option<&Value>) -> HashMap<String, FeedProjection> {
    projections
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|entries| entries.iter())
        .filter(|(key, _)| key.starts_with("nmp.feed."))
        .map(|(key, value)| (key.clone(), FeedProjection::Changed(value.clone())))
        .collect()
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

fn u64_vec_field(value: &Value, key: &str) -> Vec<u64> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect()
}

#[cfg(test)]
mod tests;
