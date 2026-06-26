//! Always-compiled signer-state FlatBuffers codec — wasm-safe, no native deps.
//!
//! This module provides the same types, encode/decode functions, and schema
//! constants as `actor::typed_projections::signer_state_fb`, but WITHOUT the
//! `#[cfg(feature = "native")]` gate that guards the whole `actor::typed_projections`
//! subtree (the actor registration builders need native thread primitives; the
//! FlatBuffers codec itself is pure data).
//!
//! Compiled only on non-native targets (`#[cfg(not(feature = "native"))]` in
//! `lib.rs`). On native targets the canonical definitions in
//! `actor::typed_projections::signer_state_fb` are used instead and re-exported
//! from the crate root under the same names.
//!
//! # Promotion rationale (#2074)
//!
//! `browser-runtime` and external consumers need to encode and decode the
//! Tier-1 signer-state typed sidecar in wasm32/browser environments, where
//! `feature = "native"` is absent. Moving only the codec (not the producer
//! registration) here is the minimal surface needed.

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
#[path = "actor/typed_projections/generated/signer_state_generated.rs"]
pub mod generated;

use flatbuffers::FlatBufferBuilder;

use generated::nmp::kernel as fb;

// Schema constants — values match the generated `signer_state_producer_consts.generated.rs`
// (which carries `pub(crate)` visibility there; we re-declare as `pub` here for
// the crate-root re-export path on non-native builds).
// These MUST stay in sync with the codegen source of truth; the codegen-drift
// CI check (`codegen-drift.yml`) validates the generated consts file.

/// Stable schema identifier carried in the typed-projection envelope.
pub const SIGNER_STATE_SCHEMA_ID: &str = "signer_state";
/// FlatBuffers file identifier embedded in every buffer produced by this codec.
pub const SIGNER_STATE_FILE_IDENTIFIER: &[u8; 4] = b"KSST";
/// Wire schema version. Bump on any breaking change to the `SignerState.fbs`.
pub const SIGNER_STATE_SCHEMA_VERSION: u32 = 1;

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
}

/// Encode a [`SignerStateModel`] to typed FlatBuffers bytes (with the `KSST`
/// file identifier). Wasm-safe — no native deps.
#[must_use]
pub fn encode_signer_state(model: &SignerStateModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let signer_kind = fbb.create_string(&model.signer_kind);
    let state = fbb.create_string(&model.state);
    let reason = model.reason.as_ref().map(|v| fbb.create_string(v));
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
        },
    );
    fb::finish_signer_state_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

/// Decode typed FlatBuffers bytes (as produced by [`encode_signer_state`]) back
/// into a [`SignerStateModel`]. Returns an error string on any malformed input.
/// D6 — total: never panics.
pub fn decode_signer_state(bytes: &[u8]) -> Result<SignerStateModel, String> {
    if bytes.len() < 8 || !fb::signer_state_buffer_has_identifier(bytes) {
        return Err("missing KSST file identifier".to_string());
    }
    let root = fb::root_as_signer_state(bytes)
        .map_err(|e| format!("not a valid SignerState buffer: {e}"))?;
    Ok(SignerStateModel {
        signer_kind: root.signer_kind().unwrap_or_default().to_string(),
        state: root.state().unwrap_or_default().to_string(),
        reason: root
            .has_reason()
            .then(|| root.reason().unwrap_or_default().to_string()),
        is_ready: root.is_ready(),
        is_awaiting_approval: root.is_awaiting_approval(),
        is_reconnecting: root.is_reconnecting(),
        is_unavailable: root.is_unavailable(),
        is_failed: root.is_failed(),
    })
}
