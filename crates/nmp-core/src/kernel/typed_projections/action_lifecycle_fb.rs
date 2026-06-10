//! Typed FlatBuffers wire codec for the kernel-owned `"action_lifecycle"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"action_lifecycle"`: `action_lifecycle_projection()` — an object
//! `{ in_flight: [LifecycleEntry], recent_terminal: [LifecycleEntry] }`. This
//! module adds a **typed FlatBuffers** encoding of the same shape, carried in the
//! `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing — the
//! generic `Value` projection.
//!
//! ## Why this codec parses a `serde_json::Value` (deviation from #1031)
//!
//! `action_lifecycle_projection()` takes `&mut self` (it runs the tracker's TTL
//! sweep on every emit) — it MUST NOT be called twice per tick. The JSON path
//! captures the produced `Value` once into a per-tick `Kernel` field; this
//! codec's [`ActionLifecycleModel`] is built by PARSING that captured `Value`.
//! The two wire forms read the SAME captured object, so they cannot diverge.
//!
//! Unlike the other Wave C built-ins in this wave this is NOT a map — it is a
//! struct with two ordered arrays (`in_flight` / `recent_terminal`), preserved in
//! producer order.
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
#[path = "generated/action_lifecycle_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub(crate) const ACTION_LIFECYCLE_SCHEMA_ID: &str = "action_lifecycle";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub(crate) const ACTION_LIFECYCLE_FILE_IDENTIFIER: &[u8; 4] = b"KALC";
/// Wire schema version. Bump on any breaking change to `action_lifecycle.fbs`.
pub(crate) const ACTION_LIFECYCLE_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of one SERIALISED `LifecycleEntry` row.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LifecycleEntryRow {
    pub(crate) correlation_id: String,
    pub(crate) stage: String,
    /// `Failed { reason }`'s reason, lifted as a sibling of `stage`.
    pub(crate) reason: Option<String>,
}

/// The `"action_lifecycle"` read model — the `{ in_flight, recent_terminal }`
/// shape.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ActionLifecycleModel {
    pub(crate) in_flight: Vec<LifecycleEntryRow>,
    pub(crate) recent_terminal: Vec<LifecycleEntryRow>,
}

/// Build an [`ActionLifecycleModel`] by PARSING the captured `action_lifecycle`
/// `serde_json::Value` (a `{ in_flight, recent_terminal }` object). A missing
/// array degrades to empty (D6).
pub(crate) fn model_from_json(value: &serde_json::Value) -> ActionLifecycleModel {
    ActionLifecycleModel {
        in_flight: parse_array(value.get("in_flight")),
        recent_terminal: parse_array(value.get("recent_terminal")),
    }
}

/// Parse an optional `[LifecycleEntry]` array.
fn parse_array(value: Option<&serde_json::Value>) -> Vec<LifecycleEntryRow> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|arr| arr.iter().map(entry_from_json).collect())
        .unwrap_or_default()
}

/// Parse one `LifecycleEntry` row object.
fn entry_from_json(row: &serde_json::Value) -> LifecycleEntryRow {
    LifecycleEntryRow {
        correlation_id: row
            .get("correlation_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
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
    }
}

// --- encode ---------------------------------------------------------------

/// Encode one [`LifecycleEntryRow`] into this module's generated
/// `LifecycleEntry` table.
fn create_lifecycle_entry<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &LifecycleEntryRow,
) -> WIPOffset<fb::LifecycleEntry<'a>> {
    let correlation_id = fbb.create_string(&row.correlation_id);
    let stage = fbb.create_string(&row.stage);
    let reason = row.reason.as_ref().map(|v| fbb.create_string(v));
    fb::LifecycleEntry::create(
        fbb,
        &fb::LifecycleEntryArgs {
            correlation_id: Some(correlation_id),
            stage: Some(stage),
            has_reason: row.reason.is_some(),
            reason,
        },
    )
}

/// Encode a vector of [`LifecycleEntryRow`] into a FlatBuffers vector offset.
fn create_entries<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    rows: &[LifecycleEntryRow],
) -> WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<fb::LifecycleEntry<'a>>>> {
    let offsets: Vec<WIPOffset<fb::LifecycleEntry<'_>>> = rows
        .iter()
        .map(|row| create_lifecycle_entry(fbb, row))
        .collect();
    fbb.create_vector(&offsets)
}

/// Encode an [`ActionLifecycleModel`] to typed FlatBuffers bytes (with the
/// `KALC` file identifier).
#[must_use]
pub(crate) fn encode_action_lifecycle(model: &ActionLifecycleModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let in_flight = create_entries(&mut fbb, &model.in_flight);
    let recent_terminal = create_entries(&mut fbb, &model.recent_terminal);
    let root = fb::ActionLifecycleSnapshot::create(
        &mut fbb,
        &fb::ActionLifecycleSnapshotArgs {
            in_flight: Some(in_flight),
            recent_terminal: Some(recent_terminal),
        },
    );
    fb::finish_action_lifecycle_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_action_lifecycle`])
/// back into an [`ActionLifecycleModel`]. Returns an error string on any
/// malformed input.
#[cfg(test)]
pub(crate) fn decode_action_lifecycle(bytes: &[u8]) -> Result<ActionLifecycleModel, String> {
    if bytes.len() < 8 || !fb::action_lifecycle_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KALC file identifier".to_string());
    }
    let root = fb::root_as_action_lifecycle_snapshot(bytes)
        .map_err(|e| format!("not a valid ActionLifecycleSnapshot buffer: {e}"))?;

    Ok(ActionLifecycleModel {
        in_flight: decode_entries(root.in_flight()),
        recent_terminal: decode_entries(root.recent_terminal()),
    })
}

/// Decode an optional generated `[LifecycleEntry]` vector into a
/// `Vec<LifecycleEntryRow>`.
#[cfg(test)]
fn decode_entries(
    fb_entries: Option<
        flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<fb::LifecycleEntry<'_>>>,
    >,
) -> Vec<LifecycleEntryRow> {
    let mut rows = Vec::new();
    if let Some(fb_entries) = fb_entries {
        rows.reserve(fb_entries.len());
        for entry in fb_entries.iter() {
            rows.push(lifecycle_entry_from_fb(entry));
        }
    }
    rows
}

/// Decode this module's generated `LifecycleEntry` table into a
/// [`LifecycleEntryRow`].
#[cfg(test)]
fn lifecycle_entry_from_fb(row: fb::LifecycleEntry<'_>) -> LifecycleEntryRow {
    LifecycleEntryRow {
        correlation_id: row.correlation_id().unwrap_or_default().to_string(),
        stage: row.stage().unwrap_or_default().to_string(),
        reason: row
            .has_reason()
            .then(|| row.reason().unwrap_or_default().to_string()),
    }
}

#[cfg(test)]
#[path = "action_lifecycle_fb_tests.rs"]
mod tests;
