//! Rust-owned signer-state projection slot for the browser runtime (#2074).
//!
//! The native path registers the `"signer_state"` typed projection through the
//! actor's `SignerStateSlot` (an `Arc<Mutex<Option<SignerStateDto>>>`). The
//! browser runtime has no actor thread, so it owns an equivalent slot
//! ([`BrowserSignerStateSlot`]) and registers the typed `"signer_state"`
//! projection closure against the `KernelReducer`'s `SnapshotRegistry` at
//! `start()`.
//!
//! # Wire parity + fail-closed clearing
//!
//! The registered closure mirrors `actor/typed_projections/mod.rs`'s
//! `signer_state_typed`: it emits a `Changed` row (typed payload via
//! [`encode_signer_state`]) while a signer is active. But unlike the native
//! path, the browser frame is consumed through the Rust-owned
//! `ProjectionMergeCache`, which RETAINS any projection key omitted from a
//! frame (omitted == unchanged, ADR-0055 Rung 3). A closure that simply
//! returned `None` on clear would therefore leave a STALE signer-state sidecar
//! in the merged frame forever, because `signer_state` is NOT one of the
//! kernel's `CONDITIONAL_PRESENCE_KEYS` (so `rung3_omit` never synthesizes a
//! `Cleared` for it).
//!
//! To clear correctly the slot carries a one-shot tombstone
//! ([`SignerStateSlotInner::needs_clear`]): when [`update_signer_state`]
//! transitions `Some → None`, the NEXT closure invocation emits an explicit
//! `Cleared` row (which `ProjectionMergeCache` honours by dropping the key),
//! then reverts to emitting nothing. Idempotent on the merge side, and robust
//! whether or not the host has declared incremental-apply.
//!
//! # Provider readiness (#2074)
//!
//! [`ready_model`] maps a [`SignerBackend`] to a `ready` [`SignerStateModel`].
//! At `start()` the runtime seeds the slot from the SOLE registered provider's
//! backend (see `CapabilityProviderRegistry::sole_backend`) so the projection
//! reflects "a signer is available and ready" rather than silently empty.
//! Browser NIP-46 handshake progress, readiness, and terminal failure also
//! write this slot through the NIP-46 runtime bridge. NIP-44 encrypt/decrypt
//! provider failures surface through the signer-port continuation rather than
//! by mutating readiness state.
//!
//! D4 — single writer: [`update_signer_state`] is the ONLY writer; the closure
//! only reads (plus resetting its own one-shot tombstone). D6 — total: a
//! poisoned read yields no sidecar (never serializes recovered poisoned data);
//! a write recovers the poison so it is NOT dropped (a dropped write would
//! leave stale-but-presented state). Neither path panics.

use std::sync::{Arc, Mutex, PoisonError};

use nmp_core::{
    encode_signer_state, SignerStateModel, TypedProjectionData, WireProjectionState,
    SIGNER_STATE_FILE_IDENTIFIER, SIGNER_STATE_SCHEMA_ID, SIGNER_STATE_SCHEMA_VERSION,
};
use nmp_signers::SignerBackend;

/// Inner slot state: the current model plus a one-shot clear tombstone.
///
/// `needs_clear` is set by [`update_signer_state`] on a `Some → None`
/// transition and consumed by the projection closure on its next run (it emits
/// exactly one explicit `Cleared` row so the merge cache drops the key).
#[derive(Default)]
pub struct SignerStateSlotInner {
    /// Current signer-state model, or `None` when no signer is active.
    model: Option<SignerStateModel>,
    /// One-shot: emit a single `Cleared` row on the next closure run.
    needs_clear: bool,
}

impl SignerStateSlotInner {
    /// Read-only view of the current model (for diagnostics). `None` when no
    /// signer is active. Does NOT expose the one-shot clear tombstone.
    #[must_use]
    pub fn model(&self) -> Option<&SignerStateModel> {
        self.model.as_ref()
    }
}

/// Browser-runtime signer-state slot.
///
/// The outer `Arc` is cloned into the snapshot-projection closure registered at
/// `start()` and held by `BrowserRuntimeHandle::set_signer_state`.
pub type BrowserSignerStateSlot = Arc<Mutex<SignerStateSlotInner>>;

/// Construct a fresh, empty `BrowserSignerStateSlot`.
#[must_use]
pub fn new_signer_state_slot() -> BrowserSignerStateSlot {
    Arc::new(Mutex::new(SignerStateSlotInner::default()))
}

/// Map a [`SignerBackend`] to a `ready` [`SignerStateModel`] (#2074).
///
/// The `signer_kind` strings mirror the native `SignerStateDto` vocabulary
/// (`"local"` / `"nip46"` / `"nip07"` / `"nip55"`); a `Custom(kind)` backend
/// carries its own kind string verbatim.
#[must_use]
pub fn ready_model(backend: &SignerBackend) -> SignerStateModel {
    let signer_kind = match backend {
        SignerBackend::LocalKey => "local".to_string(),
        SignerBackend::Nip46 => "nip46".to_string(),
        SignerBackend::Nip07 => "nip07".to_string(),
        SignerBackend::Nip55 => "nip55".to_string(),
        SignerBackend::Custom(kind) => kind.clone(),
    };
    SignerStateModel {
        signer_kind,
        state: "ready".to_string(),
        is_ready: true,
        ..Default::default()
    }
}

/// Browser NIP-46 handshake progress mapped onto the shared signer-state
/// vocabulary.
#[must_use]
pub fn nip46_progress_model(
    stage: &str,
    _code: Option<String>,
    detail: Option<String>,
) -> SignerStateModel {
    let state = stage_to_state(stage);
    SignerStateModel {
        signer_kind: "nip46".to_string(),
        is_awaiting_approval: state == "awaiting_approval",
        is_reconnecting: state == "reconnecting",
        is_ready: state == "ready",
        is_failed: state == "failed",
        state,
        reason: detail,
        ..Default::default()
    }
}

/// Terminal browser NIP-46 failure state.
#[must_use]
pub fn nip46_failed_model(reason: String) -> SignerStateModel {
    SignerStateModel {
        signer_kind: "nip46".to_string(),
        state: "failed".to_string(),
        is_failed: true,
        reason: Some(reason),
        ..Default::default()
    }
}

fn stage_to_state(stage: &str) -> String {
    match stage {
        "ready" => "ready",
        "failed" => "failed",
        "reconnecting" | "connecting" => "reconnecting",
        _ => "awaiting_approval",
    }
    .to_string()
}

/// Register the typed `"signer_state"` projection closure on `reducer`.
///
/// The closure captures `slot` by clone and runs on each
/// `KernelReducer::make_update_frame` tick:
/// - active (`model = Some`) → a `Changed` row carrying the typed payload;
/// - just-cleared (`needs_clear`) → exactly one `Cleared` row (then reverts);
/// - idle / poisoned → `None` (no sidecar this tick).
///
/// D4: the closure only reads `model` and resets its OWN one-shot tombstone;
/// [`update_signer_state`] is the sole writer of `model`. D6: a poisoned slot
/// yields `None` (no sidecar) — never serializes recovered poisoned data.
pub fn register_signer_state_projection(
    reducer: &nmp_core::KernelReducer,
    slot: BrowserSignerStateSlot,
) {
    let file_id = String::from_utf8_lossy(SIGNER_STATE_FILE_IDENTIFIER).into_owned();
    reducer.register_typed_snapshot_projection(SIGNER_STATE_SCHEMA_ID, move || {
        // D6: poisoned slot → no sidecar (None). We deliberately do NOT recover
        // and serialize poisoned data here (that would present possibly-torn
        // state); the prior good value (if any) stays in the host's merge cache.
        let mut guard = slot.lock().ok()?;
        if let Some(model) = guard.model.clone() {
            // Active: a Changed row supersedes any pending clear.
            guard.needs_clear = false;
            return Some(TypedProjectionData {
                key: SIGNER_STATE_SCHEMA_ID.to_string(),
                schema_id: SIGNER_STATE_SCHEMA_ID.to_string(),
                schema_version: SIGNER_STATE_SCHEMA_VERSION,
                file_identifier: file_id.clone(),
                payload: encode_signer_state(&model),
                // state defaults to Changed; rev stamped by make_update.
                ..Default::default()
            });
        }
        if guard.needs_clear {
            // One-shot explicit Cleared so the merge cache drops the key.
            guard.needs_clear = false;
            return Some(TypedProjectionData {
                key: SIGNER_STATE_SCHEMA_ID.to_string(),
                state: WireProjectionState::Cleared,
                ..Default::default()
            });
        }
        None
    });
}

/// Write a new signer-state model into the slot (the sole writer seam).
///
/// On a `Some → None` transition this arms the one-shot clear tombstone so the
/// next projection tick emits an explicit `Cleared` row (the merge cache then
/// drops the key — see the module doc). The next `make_update_frame` tick picks
/// up a new `Some(model)` and emits it in the `"signer_state"` typed sidecar.
///
/// D4: the only writer of `model` (called from `BrowserRuntimeHandle`
/// methods, never from inside a `pump()` turn or the projection closure).
/// D6: a poisoned slot is RECOVERED (`into_inner`) so the write is applied
/// rather than dropped — a dropped write would leave stale-but-presented state.
pub fn update_signer_state(slot: &BrowserSignerStateSlot, model: Option<SignerStateModel>) {
    let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
    match (&guard.model, &model) {
        // Some → None: arm the one-shot clear.
        (Some(_), None) => guard.needs_clear = true,
        // Any → Some: a fresh value supersedes a pending clear.
        (_, Some(_)) => guard.needs_clear = false,
        // None → None: nothing was presented; nothing to clear.
        (None, None) => {}
    }
    guard.model = model;
}
