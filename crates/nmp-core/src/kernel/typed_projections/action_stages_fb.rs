//! Typed FlatBuffers wire codec for the kernel-owned `"action_stages"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"action_stages"`: `action_stages_projection()` — an object keyed by
//! `correlation_id`, each value an ARRAY of stage rows
//! `{stage, reason?, at_ms, detail?}`. This module adds a **typed FlatBuffers**
//! encoding of the same shape, carried in the `typed_projections` sidecar
//! (ADR-0037) ALONGSIDE — never replacing — the generic `Value` projection.
//!
//! ## Why this codec parses a `serde_json::Value` (deviation from #1031)
//!
//! `take_action_results_projection()` records terminals into this mirror earlier
//! in the same tick, so its contents change across the tick; to read the EXACT
//! value the JSON key carries (and stay uniform with the four drained built-ins),
//! the JSON path captures this projection's `Value` once into a per-tick `Kernel`
//! field and this codec's [`ActionStagesModel`] is built by PARSING that captured
//! `Value`. The two wire forms read the SAME captured object, so they cannot
//! diverge.
//!
//! FlatBuffers has no map type, so the `correlation_id -> [stage]` map is
//! flattened to a key-sorted vector of `{key, stages}` entries — the serde JSON
//! map is BTree-ordered, so sorting matches the JSON key order. Inner history
//! arrays preserve producer order.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input.

// The generated FlatBuffers bindings are intrinsically `unsafe`. This `allow`
// block scopes the relaxation to the single generated module.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/action_stages_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const ACTION_STAGES_SCHEMA_ID: &str = "action_stages";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const ACTION_STAGES_FILE_IDENTIFIER: &[u8; 4] = b"KAST";
/// Wire schema version. Bump on any breaking change to `action_stages.fbs`.
pub const ACTION_STAGES_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of one SERIALISED `StageEntry` row.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActionStageEntryRow {
    pub stage: String,
    /// `Failed { reason }`'s reason, lifted as a sibling of `stage`.
    pub reason: Option<String>,
    pub at_ms: u64,
    /// Opaque per-stage detail, carried as its SERIALISED JSON string.
    pub detail: Option<String>,
}

/// The `"action_stages"` read model — the `correlation_id -> [StageEntry]` map
/// flattened to a key-sorted vector of `(key, history)` entries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActionStagesModel {
    /// `(key, history)` entries, sorted by `key` (matches the BTree JSON).
    pub entries: Vec<(String, Vec<ActionStageEntryRow>)>,
}

/// Build an [`ActionStagesModel`] by PARSING the captured `action_stages`
/// `serde_json::Value` (an object keyed by correlation_id, each value an array of
/// stage rows). Entries are sorted by key to match the BTree-ordered serde JSON
/// map. A non-object / non-array shape degrades to an empty / best-effort model
/// (D6).
pub(crate) fn model_from_json(value: &serde_json::Value) -> ActionStagesModel {
    let mut entries: Vec<(String, Vec<ActionStageEntryRow>)> = value
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(key, history)| {
                    let rows = history
                        .as_array()
                        .map(|arr| arr.iter().map(stage_entry_from_json).collect())
                        .unwrap_or_default();
                    (key.clone(), rows)
                })
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    ActionStagesModel { entries }
}

/// Parse one stage row object. The `detail` value is RE-SERIALISED back to its
/// JSON string (forwarding, not interpreting — D0).
fn stage_entry_from_json(row: &serde_json::Value) -> ActionStageEntryRow {
    ActionStageEntryRow {
        stage: row
            .get("stage")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        reason: row
            .get("reason")
            .filter(|v| !v.is_null())
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        at_ms: row
            .get("at_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        detail: row
            .get("detail")
            .filter(|v| !v.is_null())
            .map(|v| v.to_string()),
    }
}

// --- encode ---------------------------------------------------------------

/// Encode one [`ActionStageEntryRow`] into this module's generated
/// `ActionStageEntry` table.
fn create_stage_entry<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &ActionStageEntryRow,
) -> WIPOffset<fb::ActionStageEntry<'a>> {
    let stage = fbb.create_string(&row.stage);
    let reason = row.reason.as_ref().map(|v| fbb.create_string(v));
    let detail = row.detail.as_ref().map(|v| fbb.create_string(v));
    fb::ActionStageEntry::create(
        fbb,
        &fb::ActionStageEntryArgs {
            stage: Some(stage),
            has_reason: row.reason.is_some(),
            reason,
            at_ms: row.at_ms,
            has_detail: row.detail.is_some(),
            detail,
        },
    )
}

/// Encode an [`ActionStagesModel`] to typed FlatBuffers bytes (with the `KAST`
/// file identifier).
#[must_use]
pub(crate) fn encode_action_stages(model: &ActionStagesModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let entry_offsets: Vec<WIPOffset<fb::ActionStagesEntry<'_>>> = model
        .entries
        .iter()
        .map(|(key, history)| {
            let stage_offsets: Vec<WIPOffset<fb::ActionStageEntry<'_>>> = history
                .iter()
                .map(|row| create_stage_entry(&mut fbb, row))
                .collect();
            let key = fbb.create_string(key);
            let stages = fbb.create_vector(&stage_offsets);
            fb::ActionStagesEntry::create(
                &mut fbb,
                &fb::ActionStagesEntryArgs {
                    key: Some(key),
                    stages: Some(stages),
                },
            )
        })
        .collect();
    let entries = fbb.create_vector(&entry_offsets);
    let root = fb::ActionStagesSnapshot::create(
        &mut fbb,
        &fb::ActionStagesSnapshotArgs {
            entries: Some(entries),
        },
    );
    fb::finish_action_stages_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_action_stages`]) back
/// into an [`ActionStagesModel`]. Returns an error string on any malformed input.
pub fn decode_action_stages(bytes: &[u8]) -> Result<ActionStagesModel, String> {
    if bytes.len() < 8 || !fb::action_stages_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KAST file identifier".to_string());
    }
    let root = fb::root_as_action_stages_snapshot(bytes)
        .map_err(|e| format!("not a valid ActionStagesSnapshot buffer: {e}"))?;

    let mut entries = Vec::new();
    if let Some(fb_entries) = root.entries() {
        entries.reserve(fb_entries.len());
        for entry in fb_entries.iter() {
            let key = entry.key().unwrap_or_default().to_string();
            let mut history = Vec::new();
            if let Some(fb_stages) = entry.stages() {
                history.reserve(fb_stages.len());
                for stage in fb_stages.iter() {
                    history.push(stage_entry_from_fb(stage));
                }
            }
            entries.push((key, history));
        }
    }
    Ok(ActionStagesModel { entries })
}

/// Decode this module's generated `ActionStageEntry` table into an
/// [`ActionStageEntryRow`].
fn stage_entry_from_fb(row: fb::ActionStageEntry<'_>) -> ActionStageEntryRow {
    ActionStageEntryRow {
        stage: row.stage().unwrap_or_default().to_string(),
        reason: row
            .has_reason()
            .then(|| row.reason().unwrap_or_default().to_string()),
        at_ms: row.at_ms(),
        detail: row
            .has_detail()
            .then(|| row.detail().unwrap_or_default().to_string()),
    }
}

#[cfg(test)]
#[path = "action_stages_fb_tests.rs"]
mod tests;
