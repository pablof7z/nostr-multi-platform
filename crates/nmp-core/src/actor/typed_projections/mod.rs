//! Tier-1 (closure-path) typed-projection codecs for the actor-owned remote-
//! signer built-in projections `"bunker_handshake"`, `"nip46_onboarding"`, and
//! `"signer_state"` (ADR-0048 D6 — generalised from the former
//! `"bunker_connection_state"` projection, V-14 step b / #963 / #1098).
//!
//! ## Why these live under `actor/` (not `kernel/typed_projections/`)
//!
//! The kernel-owned (Tier-2) built-ins are inserted directly by `make_update`
//! and typed via `Kernel::builtin_typed_projections`. These two projections are
//! different: they are registered through the **`SnapshotRegistry` closure
//! path** in [`crate::actor`] (`registry.register("bunker_handshake", …)` /
//! `registry.register("nip46_onboarding", …)`), reading shared actor-owned
//! slots — not live `&self` kernel state. So they are typed the **Tier-1 way**:
//! a `registry.register_typed(key, closure -> Option<TypedProjectionData>)`
//! registered ALONGSIDE the existing generic `registry.register(...)`. The
//! closure encodes the typed FlatBuffer from the SAME slot the JSON closure
//! reads, so the two wire forms cannot diverge.
//!
//! Because registration is actor-side, the codecs and their slot→envelope
//! builders live under `actor/` (reachable from `actor/mod.rs`) rather than the
//! private `kernel::typed_projections` module.
//!
//! ## Conditional presence (mirror the JSON closure's `Some`/`None`)
//!
//! - `"bunker_handshake"` is **conditionally present**: the JSON closure emits
//!   `null` when the shared slot is `None` (no handshake in flight). The typed
//!   builder returns `None` in that case (no sidecar entry), and `Some(envelope)`
//!   only when the slot is `Some`.
//! - `"nip46_onboarding"` is **always present**: `build_nip46_onboarding_dto`
//!   always returns a value (the static signer-app probe table is emitted even
//!   when idle), so the typed builder always returns `Some(envelope)`.
//!
//! D0: NIP-46 remote signing is an app noun — both projections live under the
//! kernel's `projections` map (never as typed `KernelSnapshot` fields) and are
//! typed here ALONGSIDE their generic `Value`. D6: every `decode_*` returns
//! `Err(String)` on malformed input; the encode path never panics.

mod bunker_handshake_fb;
mod nip46_onboarding_fb;
mod signer_state_fb;

use crate::actor::commands::{build_nip46_onboarding_dto, BunkerHandshakeSlot, SignerStateSlot};
use crate::update_envelope::TypedProjectionData;

// Re-exported: encoders + schema ID constants + model types (production), and
// the decode functions.  `pub(crate)` for the encoder/constants (internal
// production path) and `pub` for the decode surface + model types so external
// crates (chirp-desktop, Android shell) can decode typed sidecar frames.
pub(crate) use bunker_handshake_fb::{
    encode_bunker_handshake, BUNKER_HANDSHAKE_FILE_IDENTIFIER, BUNKER_HANDSHAKE_SCHEMA_VERSION,
};
pub(crate) use nip46_onboarding_fb::{
    encode_nip46_onboarding, NIP46_ONBOARDING_FILE_IDENTIFIER, NIP46_ONBOARDING_SCHEMA_VERSION,
};
pub(crate) use signer_state_fb::{
    encode_signer_state, SIGNER_STATE_FILE_IDENTIFIER, SIGNER_STATE_SCHEMA_VERSION,
};
// Promoted from #[cfg(test)]: decode functions + model types + schema IDs are
// now public so external shells (e.g. chirp-desktop, Android) can decode the
// "signer_state", "bunker_handshake", and "nip46_onboarding" typed sidecars.
pub use bunker_handshake_fb::{
    decode_bunker_handshake, BunkerHandshakeModel, BUNKER_HANDSHAKE_SCHEMA_ID,
};
pub use nip46_onboarding_fb::{
    decode_nip46_onboarding, Nip46OnboardingModel, SignerAppRow, NIP46_ONBOARDING_SCHEMA_ID,
};
pub use signer_state_fb::{decode_signer_state, SignerStateModel, SIGNER_STATE_SCHEMA_ID};

/// Build the typed `"signer_state"` sidecar entry from the shared slot.
///
/// ADR-0048 D6 — generalises the former `"bunker_connection_state"` (V-14 step
/// b) into a unified remote-signer health surface. Both NIP-46 and NIP-55
/// write into the same slot via `IdentityRuntime::set_signer_state`. The typed
/// payload is a field-for-field encode of the full `SignerStateDto`
/// (`signer_state.fbs`, file identifier `KSST`).
///
/// Returns `Some(envelope)` ONLY when the slot holds `Some` (mirroring the JSON
/// closure, which emits `null` — and thus no typed sidecar entry — while no
/// remote signer is active). Registered via
/// `registry.register_typed("signer_state", …)` ALONGSIDE the generic closure in
/// [`crate::actor`]. Reads the SAME slot, so the typed and JSON forms cannot
/// diverge. D8: a single lock-and-clone, non-blocking.
pub(crate) fn signer_state_typed(slot: &SignerStateSlot) -> Option<TypedProjectionData> {
    let dto = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()?;
    let model = SignerStateModel {
        signer_kind: dto.signer_kind.clone(),
        state: dto.state.clone(),
        reason: dto.reason.clone(),
        is_ready: dto.is_ready,
        is_awaiting_approval: dto.is_awaiting_approval,
        is_reconnecting: dto.is_reconnecting,
        is_unavailable: dto.is_unavailable,
        is_failed: dto.is_failed,
        status_label: dto.status_label.clone(),
        status_tone: dto.status_tone.clone(),
    };
    Some(TypedProjectionData {
        key: SIGNER_STATE_SCHEMA_ID.to_string(),
        schema_id: SIGNER_STATE_SCHEMA_ID.to_string(),
        schema_version: SIGNER_STATE_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(SIGNER_STATE_FILE_IDENTIFIER).into_owned(),
        payload: encode_signer_state(&model),
        // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
        ..Default::default()
    })
}

/// Build the typed `"bunker_handshake"` sidecar entry from the shared slot.
///
/// Returns `Some(envelope)` ONLY when the slot holds `Some` (mirroring the JSON
/// closure, which emits `null` — and thus no typed sidecar entry — while idle).
/// Registered via `registry.register_typed("bunker_handshake", …)` ALONGSIDE the
/// generic closure in [`crate::actor`]. Reads the SAME slot, so the typed and
/// JSON forms cannot diverge. D8: a single lock-and-clone, non-blocking.
pub(crate) fn bunker_handshake_typed(slot: &BunkerHandshakeSlot) -> Option<TypedProjectionData> {
    // D6: a poisoned bunker-handshake mutex recovers via `into_inner` rather
    // than panicking inside the snapshot tick — matching the generic closure.
    let dto = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()?;
    let model = BunkerHandshakeModel {
        stage: dto.stage,
        message: dto.message,
        is_idle: dto.is_idle,
        is_in_flight: dto.is_in_flight,
        is_failed: dto.is_failed,
        is_terminal_success: dto.is_terminal_success,
        can_cancel: dto.can_cancel,
        stage_label: dto.stage_label,
    };
    Some(TypedProjectionData {
        key: BUNKER_HANDSHAKE_SCHEMA_ID.to_string(),
        schema_id: BUNKER_HANDSHAKE_SCHEMA_ID.to_string(),
        schema_version: BUNKER_HANDSHAKE_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(BUNKER_HANDSHAKE_FILE_IDENTIFIER).into_owned(),
        payload: encode_bunker_handshake(&model),
        // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
        ..Default::default()
    })
}

/// Build the typed `"nip46_onboarding"` sidecar entry from the shared slot.
///
/// ALWAYS returns `Some(envelope)` — `build_nip46_onboarding_dto` always
/// produces a value (the static signer-app probe table), mirroring the JSON
/// closure (never `null`). Registered via
/// `registry.register_typed("nip46_onboarding", …)` ALONGSIDE the generic
/// closure in [`crate::actor`]. Reads the SAME slot (via the SAME
/// `build_nip46_onboarding_dto`), so the typed and JSON forms cannot diverge.
pub(crate) fn nip46_onboarding_typed(slot: &BunkerHandshakeSlot) -> Option<TypedProjectionData> {
    let dto = build_nip46_onboarding_dto(slot);
    // Derive the `stage_kind` wire token through serde so the typed buffer
    // carries the EXACT snake_case string the JSON projection emits — no
    // hand-maintained mapping that could drift from the enum's
    // `rename_all = "snake_case"` derive. `None` (no handshake in flight) stays
    // `None`; a `Some(kind)` serialises to a JSON string we read back out.
    let stage_kind = serde_json::to_value(&dto.stage_kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned));
    let model = Nip46OnboardingModel {
        signer_apps: dto
            .signer_apps
            .into_iter()
            .map(|app| SignerAppRow {
                scheme: app.scheme,
                display_label: app.display_label,
                signer_kind: app.signer_kind,
            })
            .collect(),
        stage_kind,
        progress_message: dto.progress_message,
        is_in_flight: dto.is_in_flight,
        is_failed: dto.is_failed,
        is_terminal_success: dto.is_terminal_success,
        can_cancel: dto.can_cancel,
    };
    Some(TypedProjectionData {
        key: NIP46_ONBOARDING_SCHEMA_ID.to_string(),
        schema_id: NIP46_ONBOARDING_SCHEMA_ID.to_string(),
        schema_version: NIP46_ONBOARDING_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(NIP46_ONBOARDING_FILE_IDENTIFIER).into_owned(),
        payload: encode_nip46_onboarding(&model),
        // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
        ..Default::default()
    })
}

#[cfg(test)]
#[path = "typed_projections_tests.rs"]
mod tests;
