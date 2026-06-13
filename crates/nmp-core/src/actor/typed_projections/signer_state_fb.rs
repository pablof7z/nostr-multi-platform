//! Typed FlatBuffers wire codec for the actor-owned `"signer_state"`
//! projection (Tier-1 closure path). ADR-0048 D6 — generalises the former
//! `"bunker_connection_state"` codec (V-14 step b, #963 / #1098, file
//! identifier "KBCS") into the unified remote-signer health surface. The
//! rename is a hard break (no-compat-alias rule): the old schema, table, and
//! identifier are gone; all in-repo consumers read `"signer_state"` / `KSST`.
//!
//! The authoritative FFI shape is the serde JSON the
//! `registry.register("signer_state", …)` closure in
//! `crates/nmp-core/src/actor/mod.rs` inserts under `"signer_state"`: the
//! serialisation of the shared `SignerStateSlot`
//! (`Arc<Mutex<Option<SignerStateDto>>>`) — JSON `null` when the slot is
//! `None`, else the serialised `SignerStateDto`. This module adds a **typed
//! FlatBuffers** encoding of the same shape, carried in the
//! `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing — the
//! generic `Value` projection, and only when the slot holds `Some` (the typed
//! closure mirrors the JSON closure's `Some`/`None`: no sidecar entry while
//! the slot is idle).
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
#[path = "generated/signer_state_generated.rs"]
pub mod generated;

use flatbuffers::FlatBufferBuilder;

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const SIGNER_STATE_SCHEMA_ID: &str = "signer_state";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub(crate) const SIGNER_STATE_FILE_IDENTIFIER: &[u8; 4] = b"KSST";
/// Wire schema version. Bump on any breaking change to `signer_state.fbs`.
pub(crate) const SIGNER_STATE_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of the serialised `SignerStateDto` — the value the
/// `"signer_state"` JSON projection serialises when the slot is `Some`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SignerStateModel {
    /// `"nip46"` | `"nip55"` | `"local"`.
    pub signer_kind: String,
    /// `"ready"` | `"awaiting_approval"` | `"reconnecting"` | `"unavailable"`
    /// | `"failed"`.
    pub state: String,
    /// Optional human-readable reason (error message on degraded states).
    pub reason: Option<String>,
    /// `state == "ready"`.
    pub is_ready: bool,
    /// `state == "awaiting_approval"` (NIP-55 Intent round-trip in flight).
    pub is_awaiting_approval: bool,
    /// `state == "reconnecting"`.
    pub is_reconnecting: bool,
    /// `state == "unavailable"` (NIP-55 signer app missing).
    pub is_unavailable: bool,
    /// `state == "failed"`.
    pub is_failed: bool,
    /// Pre-computed display label (ADR-0032 / #1099). Rendered verbatim by
    /// shells; mirrors `SignerStateDto::status_label`.
    pub status_label: String,
    /// Pre-computed display tone — `"active"` | `"warning"` | `"error"` |
    /// `"inactive"` (ADR-0032 / #1099). Mirrors `SignerStateDto::status_tone`.
    pub status_tone: String,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`SignerStateModel`] to typed FlatBuffers bytes (with the `KSST`
/// file identifier).
#[must_use]
pub(crate) fn encode_signer_state(model: &SignerStateModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let signer_kind = fbb.create_string(&model.signer_kind);
    let state = fbb.create_string(&model.state);
    let reason = model.reason.as_ref().map(|v| fbb.create_string(v));
    let status_label = fbb.create_string(&model.status_label);
    let status_tone = fbb.create_string(&model.status_tone);
    let root = fb::SignerState::create(
        &mut fbb,
        &fb::SignerStateArgs {
            signer_kind: Some(signer_kind),
            state: Some(state),
            has_reason: model.reason.is_some(),
            reason,
            is_ready: model.is_ready,
            is_awaiting_approval: model.is_awaiting_approval,
            is_reconnecting: model.is_reconnecting,
            is_unavailable: model.is_unavailable,
            is_failed: model.is_failed,
            status_label: Some(status_label),
            status_tone: Some(status_tone),
        },
    );
    fb::finish_signer_state_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_signer_state`])
/// back into a [`SignerStateModel`]. Returns an error string on any malformed
/// input. Available to sibling crates (e.g. desktop shells) for decoding the
/// `"signer_state"` typed sidecar from a snapshot frame.
pub fn decode_signer_state(bytes: &[u8]) -> Result<SignerStateModel, String> {
    if bytes.len() < 8 || !fb::signer_state_buffer_has_identifier(bytes) {
        return Err("missing KSST file identifier".to_string());
    }
    let root = fb::root_as_signer_state(bytes)
        .map_err(|e| format!("not a valid SignerState buffer: {e}"))?;
    let state = root.state().unwrap_or_default().to_string();
    // `status_label` / `status_tone` are tail-appended (additive) and therefore
    // absent in buffers that predate #1099 — fall back to re-deriving them from
    // the authoritative `state` token via the exact same logic the producer
    // uses, so an older buffer still yields a correct domain value (D1).
    let (fallback_label, fallback_tone) =
        crate::actor::commands::signer_state_label_and_tone(&state);
    Ok(SignerStateModel {
        signer_kind: root.signer_kind().unwrap_or_default().to_string(),
        state,
        reason: root
            .has_reason()
            .then(|| root.reason().unwrap_or_default().to_string()),
        is_ready: root.is_ready(),
        is_awaiting_approval: root.is_awaiting_approval(),
        is_reconnecting: root.is_reconnecting(),
        is_unavailable: root.is_unavailable(),
        is_failed: root.is_failed(),
        status_label: root
            .status_label()
            .map(str::to_string)
            .unwrap_or(fallback_label),
        status_tone: root
            .status_tone()
            .map(str::to_string)
            .unwrap_or(fallback_tone),
    })
}

#[cfg(test)]
#[path = "signer_state_fb_tests.rs"]
mod tests;
