//! Typed FlatBuffers wire codec for the kernel-owned `"signed_events"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"signed_events"` when a `SignEventForReturn` settled this tick: the DRAINED
//! `take_signed_events_projection()` object keyed by `correlation_id`, each value
//! `{ "ok": true, "signed_json": "…" }` or `{ "ok": false, "error": "…" }`. This
//! module adds a **typed FlatBuffers** encoding of the same shape, carried in the
//! `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing — the
//! generic `Value` projection.
//!
//! ## Why this codec parses a `serde_json::Value` (deviation from #1031)
//!
//! `take_signed_events_projection()` `clear()`s the map (the host reads each id
//! once) — it MUST NOT be called twice per tick. The JSON path captures the
//! drained `Value` once into a per-tick `Kernel` field; this codec's
//! [`SignedEventsModel`] is built by PARSING that captured `Value`. The two wire
//! forms read the SAME captured object, so they cannot diverge.
//!
//! FlatBuffers has no map type, so the map is flattened to a key-sorted vector of
//! `{key, value}` entries — the serde JSON map is BTree-ordered, so sorting
//! matches the JSON key order.
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
#[path = "generated/signed_events_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const SIGNED_EVENTS_SCHEMA_ID: &str = "signed_events";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const SIGNED_EVENTS_FILE_IDENTIFIER: &[u8; 4] = b"KSEV";
/// Wire schema version. Bump on any breaking change to `signed_events.fbs`.
pub const SIGNED_EVENTS_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of one SERIALISED `signed_events` map value.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SignedEventRow {
    pub correlation_id: String,
    pub ok: bool,
    pub signed_json: Option<String>,
    pub error: Option<String>,
}

/// The `"signed_events"` read model — the `correlation_id -> value` map
/// flattened to a key-sorted vector of `(key, value)` entries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SignedEventsModel {
    /// `(key, value)` entries, sorted by `key` (matches the BTree-ordered JSON).
    pub entries: Vec<(String, SignedEventRow)>,
}

/// Build a [`SignedEventsModel`] by PARSING the captured `signed_events`
/// `serde_json::Value` (a JSON object keyed by correlation_id). Entries are
/// sorted by key to match the BTree-ordered serde JSON map. A non-object value
/// degrades to an empty model (D6).
pub(crate) fn model_from_json(value: &serde_json::Value) -> SignedEventsModel {
    let mut entries: Vec<(String, SignedEventRow)> = value
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(key, v)| (key.clone(), row_from_json(key, v)))
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    SignedEventsModel { entries }
}

/// Parse one map value object into a [`SignedEventRow`], stamping the
/// `correlation_id` from the map key.
fn row_from_json(correlation_id: &str, value: &serde_json::Value) -> SignedEventRow {
    SignedEventRow {
        correlation_id: correlation_id.to_string(),
        ok: value
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        signed_json: value
            .get("signed_json")
            .filter(|v| !v.is_null())
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        error: value
            .get("error")
            .filter(|v| !v.is_null())
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    }
}

// --- encode ---------------------------------------------------------------

/// Encode one [`SignedEventRow`] into this module's generated `SignedEvent`
/// table.
fn create_signed_event<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &SignedEventRow,
) -> WIPOffset<fb::SignedEvent<'a>> {
    let correlation_id = fbb.create_string(&row.correlation_id);
    let signed_json = row.signed_json.as_ref().map(|v| fbb.create_string(v));
    let error = row.error.as_ref().map(|v| fbb.create_string(v));
    fb::SignedEvent::create(
        fbb,
        &fb::SignedEventArgs {
            correlation_id: Some(correlation_id),
            ok: row.ok,
            has_signed_json: row.signed_json.is_some(),
            signed_json,
            has_error: row.error.is_some(),
            error,
        },
    )
}

/// Encode a [`SignedEventsModel`] to typed FlatBuffers bytes (with the `KSEV`
/// file identifier).
#[must_use]
pub(crate) fn encode_signed_events(model: &SignedEventsModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let entry_offsets: Vec<WIPOffset<fb::SignedEventEntry<'_>>> = model
        .entries
        .iter()
        .map(|(key, row)| {
            let key = fbb.create_string(key);
            let value = create_signed_event(&mut fbb, row);
            fb::SignedEventEntry::create(
                &mut fbb,
                &fb::SignedEventEntryArgs {
                    key: Some(key),
                    value: Some(value),
                },
            )
        })
        .collect();
    let entries = fbb.create_vector(&entry_offsets);
    let root = fb::SignedEventsSnapshot::create(
        &mut fbb,
        &fb::SignedEventsSnapshotArgs {
            entries: Some(entries),
        },
    );
    fb::finish_signed_events_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_signed_events`]) back
/// into a [`SignedEventsModel`]. Returns an error string on any malformed input.
///
/// PR-B final: unconditionally compiled (no longer `#[cfg(test)]`) — promoted
/// to the public typed-decode surface so out-of-crate consumers read the
/// typed sidecar instead of the deleted JSON payload.
pub fn decode_signed_events(bytes: &[u8]) -> Result<SignedEventsModel, String> {
    if bytes.len() < 8 || !fb::signed_events_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KSEV file identifier".to_string());
    }
    let root = fb::root_as_signed_events_snapshot(bytes)
        .map_err(|e| format!("not a valid SignedEventsSnapshot buffer: {e}"))?;

    let mut entries = Vec::new();
    if let Some(fb_entries) = root.entries() {
        entries.reserve(fb_entries.len());
        for entry in fb_entries.iter() {
            let key = entry.key().unwrap_or_default().to_string();
            let value = entry.value().map(signed_event_from_fb).unwrap_or_default();
            entries.push((key, value));
        }
    }
    Ok(SignedEventsModel { entries })
}

/// Decode this module's generated `SignedEvent` table into a [`SignedEventRow`].
fn signed_event_from_fb(row: fb::SignedEvent<'_>) -> SignedEventRow {
    SignedEventRow {
        correlation_id: row.correlation_id().unwrap_or_default().to_string(),
        ok: row.ok(),
        signed_json: row
            .has_signed_json()
            .then(|| row.signed_json().unwrap_or_default().to_string()),
        error: row
            .has_error()
            .then(|| row.error().unwrap_or_default().to_string()),
    }
}

#[cfg(test)]
#[path = "signed_events_fb_tests.rs"]
mod tests;
