//! Typed FlatBuffers wire codec for the actor-owned `"bunker_handshake"`
//! projection (Tier-1 closure path).
//!
//! The authoritative FFI shape is the serde JSON the
//! `registry.register("bunker_handshake", …)` closure in
//! `crates/nmp-core/src/actor/mod.rs` inserts under `"bunker_handshake"`: the
//! serialisation of the shared `BunkerHandshakeSlot`
//! (`Arc<Mutex<Option<BunkerHandshakeDto>>>`) — JSON `null` when the slot is
//! `None`, else the serialised [`BunkerHandshakeDto`]. This module adds a
//! **typed FlatBuffers** encoding of the same shape, carried in the
//! `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing — the
//! generic `Value` projection, and only when the slot holds `Some` (the typed
//! closure mirrors the JSON closure's `Some`/`None`: no sidecar entry while the
//! slot is idle).
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
#[path = "generated/bunker_handshake_generated.rs"]
pub mod generated;

use flatbuffers::FlatBufferBuilder;

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const BUNKER_HANDSHAKE_SCHEMA_ID: &str = "bunker_handshake";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub(crate) const BUNKER_HANDSHAKE_FILE_IDENTIFIER: &[u8; 4] = b"KBHS";
/// Wire schema version. Bump on any breaking change to `bunker_handshake.fbs`.
pub(crate) const BUNKER_HANDSHAKE_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of the SERIALISED `BunkerHandshakeDto` — the value
/// the `"bunker_handshake"` JSON projection serialises when the slot is `Some`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BunkerHandshakeModel {
    pub stage: String,
    pub message: Option<String>,
    pub is_idle: bool,
    pub is_in_flight: bool,
    pub is_failed: bool,
    pub is_terminal_success: bool,
    pub can_cancel: bool,
    pub stage_label: String,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`BunkerHandshakeModel`] to typed FlatBuffers bytes (with the `KBHS`
/// file identifier).
#[must_use]
pub(crate) fn encode_bunker_handshake(model: &BunkerHandshakeModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let stage = fbb.create_string(&model.stage);
    let message = model.message.as_ref().map(|v| fbb.create_string(v));
    let stage_label = fbb.create_string(&model.stage_label);
    let root = fb::BunkerHandshake::create(
        &mut fbb,
        &fb::BunkerHandshakeArgs {
            stage: Some(stage),
            has_message: model.message.is_some(),
            message,
            is_idle: model.is_idle,
            is_in_flight: model.is_in_flight,
            is_failed: model.is_failed,
            is_terminal_success: model.is_terminal_success,
            can_cancel: model.can_cancel,
            stage_label: Some(stage_label),
        },
    );
    fb::finish_bunker_handshake_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_bunker_handshake`])
/// back into a [`BunkerHandshakeModel`]. Returns an error string on any
/// malformed input. Available to sibling crates (e.g. desktop shells) for
/// decoding the `"bunker_handshake"` typed sidecar from a snapshot frame.
pub fn decode_bunker_handshake(bytes: &[u8]) -> Result<BunkerHandshakeModel, String> {
    if bytes.len() < 8 || !fb::bunker_handshake_buffer_has_identifier(bytes) {
        return Err("missing KBHS file identifier".to_string());
    }
    let root = fb::root_as_bunker_handshake(bytes)
        .map_err(|e| format!("not a valid BunkerHandshake buffer: {e}"))?;
    Ok(BunkerHandshakeModel {
        stage: root.stage().unwrap_or_default().to_string(),
        message: root
            .has_message()
            .then(|| root.message().unwrap_or_default().to_string()),
        is_idle: root.is_idle(),
        is_in_flight: root.is_in_flight(),
        is_failed: root.is_failed(),
        is_terminal_success: root.is_terminal_success(),
        can_cancel: root.can_cancel(),
        stage_label: root.stage_label().unwrap_or_default().to_string(),
    })
}

#[cfg(test)]
#[path = "bunker_handshake_fb_tests.rs"]
mod tests;
