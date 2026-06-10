//! Typed FlatBuffers wire codec for the kernel-owned `"action_results"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"action_results"` when an action settled this tick: the DRAINED
//! `take_action_results_projection()` array of `{correlation_id, status, error,
//! result?}` rows. This module adds a **typed FlatBuffers** encoding of the same
//! shape, carried in the `typed_projections` sidecar (ADR-0037) ALONGSIDE —
//! never replacing — the generic `Value` projection.
//!
//! ## Why this codec parses a `serde_json::Value` (deviation from #1031)
//!
//! `take_action_results_projection()` is a DRAIN with side effects — it MUST NOT
//! be called twice per tick (see `action_results.fbs`). The JSON path captures
//! the drained `Value` once into a per-tick `Kernel` field; this codec's
//! [`ActionResultsModel`] is built by PARSING that captured `Value` (there is no
//! surviving DTO to map). [`model_from_json`] performs that parse; the two wire
//! forms read the SAME captured array, so they cannot diverge.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input;
//! the JSON parse degrades field-by-field (a missing field → its default / `None`)
//! rather than panicking.

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
#[path = "generated/action_results_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const ACTION_RESULTS_SCHEMA_ID: &str = "action_results";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const ACTION_RESULTS_FILE_IDENTIFIER: &[u8; 4] = b"KARS";
/// Wire schema version. Bump on any breaking change to `action_results.fbs`.
pub const ACTION_RESULTS_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of one SERIALISED action-result row.
///
/// Public surface (re-exported via `nmp_core::typed_projections`) — external
/// Rust consumers read these fields directly instead of string-keying the
/// generic JSON `payload`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActionResultRow {
    /// Correlation id of the dispatched action this row reports on.
    pub correlation_id: String,
    /// Terminal status string (e.g. `"published"`).
    pub status: String,
    /// Error string when the action failed; `None` on success.
    pub error: Option<String>,
    /// The opaque structured result body (ADR-0043 Decision 4), carried as its
    /// SERIALISED JSON string. `None` mirrors an absent `result` field.
    pub result: Option<String>,
}

/// The `"action_results"` read model — the drained array of rows in producer
/// order.
///
/// Public surface (re-exported via `nmp_core::typed_projections`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActionResultsModel {
    /// Settled action-result rows in producer order.
    pub results: Vec<ActionResultRow>,
}

/// Build an [`ActionResultsModel`] by PARSING the captured `action_results`
/// `serde_json::Value` (a JSON array). The JSON path inserts this exact value
/// under the snapshot key, so the typed form reads the same source. A non-array
/// value (or a malformed row) degrades to an empty / best-effort model rather
/// than panicking (D6).
pub(crate) fn model_from_json(value: &serde_json::Value) -> ActionResultsModel {
    let results = value
        .as_array()
        .map(|rows| rows.iter().map(row_from_json).collect())
        .unwrap_or_default();
    ActionResultsModel { results }
}

/// Parse one row object. The `result` value is RE-SERIALISED back to its JSON
/// string (forwarding, not interpreting — D0); a non-object row degrades to a
/// default row.
fn row_from_json(row: &serde_json::Value) -> ActionResultRow {
    ActionResultRow {
        correlation_id: row
            .get("correlation_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        status: row
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        // `error` serialises as `null`-when-`None` (key present); treat both an
        // absent key and a JSON `null` as `None`.
        error: row
            .get("error")
            .filter(|v| !v.is_null())
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        // `result` is omitted when absent; carry the serialised body verbatim.
        result: row
            .get("result")
            .filter(|v| !v.is_null())
            .map(|v| v.to_string()),
    }
}

// --- encode ---------------------------------------------------------------

/// Encode one [`ActionResultRow`] into this module's generated `ActionResult`
/// table.
fn create_action_result<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &ActionResultRow,
) -> WIPOffset<fb::ActionResult<'a>> {
    let correlation_id = fbb.create_string(&row.correlation_id);
    let status = fbb.create_string(&row.status);
    let error = row.error.as_ref().map(|v| fbb.create_string(v));
    let result = row.result.as_ref().map(|v| fbb.create_string(v));
    fb::ActionResult::create(
        fbb,
        &fb::ActionResultArgs {
            correlation_id: Some(correlation_id),
            status: Some(status),
            has_error: row.error.is_some(),
            error,
            has_result: row.result.is_some(),
            result,
        },
    )
}

/// Encode an [`ActionResultsModel`] to typed FlatBuffers bytes (with the `KARS`
/// file identifier).
#[must_use]
pub(crate) fn encode_action_results(model: &ActionResultsModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let row_offsets: Vec<WIPOffset<fb::ActionResult<'_>>> = model
        .results
        .iter()
        .map(|row| create_action_result(&mut fbb, row))
        .collect();
    let results = fbb.create_vector(&row_offsets);
    let root = fb::ActionResultsSnapshot::create(
        &mut fbb,
        &fb::ActionResultsSnapshotArgs {
            results: Some(results),
        },
    );
    fb::finish_action_results_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_action_results`])
/// back into an [`ActionResultsModel`]. Returns an error string on any malformed
/// input.
///
/// Public surface (re-exported via `nmp_core::typed_projections`): the
/// reachable decode entry point for the `action_results` sidecar key.
pub fn decode_action_results(bytes: &[u8]) -> Result<ActionResultsModel, String> {
    if bytes.len() < 8 || !fb::action_results_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KARS file identifier".to_string());
    }
    let root = fb::root_as_action_results_snapshot(bytes)
        .map_err(|e| format!("not a valid ActionResultsSnapshot buffer: {e}"))?;

    let mut results = Vec::new();
    if let Some(fb_results) = root.results() {
        results.reserve(fb_results.len());
        for row in fb_results.iter() {
            results.push(action_result_from_fb(row));
        }
    }
    Ok(ActionResultsModel { results })
}

/// Decode this module's generated `ActionResult` table into an
/// [`ActionResultRow`].
fn action_result_from_fb(row: fb::ActionResult<'_>) -> ActionResultRow {
    ActionResultRow {
        correlation_id: row.correlation_id().unwrap_or_default().to_string(),
        status: row.status().unwrap_or_default().to_string(),
        error: row
            .has_error()
            .then(|| row.error().unwrap_or_default().to_string()),
        result: row
            .has_result()
            .then(|| row.result().unwrap_or_default().to_string()),
    }
}

#[cfg(test)]
#[path = "action_results_fb_tests.rs"]
mod tests;
