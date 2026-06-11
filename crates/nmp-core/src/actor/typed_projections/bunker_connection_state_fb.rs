//! Typed FlatBuffers wire codec for the actor-owned `"bunker_connection_state"`
//! projection (Tier-1 closure path). V-14 step b — closes #963.
//!
//! The authoritative FFI shape is the serde JSON the
//! `registry.register("bunker_connection_state", …)` closure in
//! `crates/nmp-core/src/actor/mod.rs` inserts under
//! `"bunker_connection_state"`: the serialisation of the shared
//! `BunkerConnectionStateSlot` (`Arc<Mutex<Option<BunkerConnectionStateDto>>>`)
//! — JSON `null` when the slot is `None`, else the serialised
//! [`BunkerConnectionStateDto`]. This module adds a **typed FlatBuffers**
//! encoding of the same shape, carried in the `typed_projections` sidecar
//! (ADR-0037) ALONGSIDE — never replacing — the generic `Value` projection,
//! and only when the slot holds `Some` (the typed closure mirrors the JSON
//! closure's `Some`/`None`: no sidecar entry while the slot is idle).
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
#[path = "generated/bunker_connection_state_generated.rs"]
pub mod generated;

use flatbuffers::FlatBufferBuilder;

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub(crate) const BUNKER_CONNECTION_STATE_SCHEMA_ID: &str = "bunker_connection_state";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub(crate) const BUNKER_CONNECTION_STATE_FILE_IDENTIFIER: &[u8; 4] = b"KBCS";
/// Wire schema version. Bump on any breaking change to `bunker_connection_state.fbs`.
pub(crate) const BUNKER_CONNECTION_STATE_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of the serialised `BunkerConnectionStateDto` — the
/// value the `"bunker_connection_state"` JSON projection serialises when the slot
/// is `Some`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct BunkerConnectionStateModel {
    /// `"connected"` | `"reconnecting"` | `"failed"`.
    pub(crate) state: String,
    /// Optional human-readable reason (error message on reconnecting/failed).
    pub(crate) reason: Option<String>,
    /// `state == "connected"`.
    pub(crate) is_connected: bool,
    /// `state == "reconnecting"`.
    pub(crate) is_reconnecting: bool,
    /// `state == "failed"`.
    pub(crate) is_failed: bool,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`BunkerConnectionStateModel`] to typed FlatBuffers bytes (with the
/// `KBCS` file identifier).
#[must_use]
pub(crate) fn encode_bunker_connection_state(model: &BunkerConnectionStateModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let state = fbb.create_string(&model.state);
    let reason = model.reason.as_ref().map(|v| fbb.create_string(v));
    let root = fb::BunkerConnectionState::create(
        &mut fbb,
        &fb::BunkerConnectionStateArgs {
            state: Some(state),
            has_reason: model.reason.is_some(),
            reason,
            is_connected: model.is_connected,
            is_reconnecting: model.is_reconnecting,
            is_failed: model.is_failed,
        },
    );
    fb::finish_bunker_connection_state_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by
/// [`encode_bunker_connection_state`]) back into a
/// [`BunkerConnectionStateModel`]. Returns an error string on any malformed
/// input. Used by in-crate tests to verify round-trip integrity.
#[cfg(test)]
pub(crate) fn decode_bunker_connection_state(
    bytes: &[u8],
) -> Result<BunkerConnectionStateModel, String> {
    if bytes.len() < 8 || !fb::bunker_connection_state_buffer_has_identifier(bytes) {
        return Err("missing KBCS file identifier".to_string());
    }
    let root = fb::root_as_bunker_connection_state(bytes)
        .map_err(|e| format!("not a valid BunkerConnectionState buffer: {e}"))?;
    Ok(BunkerConnectionStateModel {
        state: root.state().unwrap_or_default().to_string(),
        reason: root
            .has_reason()
            .then(|| root.reason().unwrap_or_default().to_string()),
        is_connected: root.is_connected(),
        is_reconnecting: root.is_reconnecting(),
        is_failed: root.is_failed(),
    })
}

#[cfg(test)]
#[path = "bunker_connection_state_fb_tests.rs"]
mod tests;
