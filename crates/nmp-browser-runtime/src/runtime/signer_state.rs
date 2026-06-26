//! Rust-owned signer-state projection slot for the browser runtime (#2074).
//!
//! The native path registers the `"signer_state"` typed projection through the
//! actor's `SignerStateSlot` (an `Arc<Mutex<Option<SignerStateDto>>>`). The
//! browser runtime has no actor thread, so it owns an equivalent
//! `BrowserSignerStateSlot` (`Arc<Mutex<Option<SignerStateModel>>>`) and
//! registers the typed `"signer_state"` projection closure against the
//! `KernelReducer`'s `SnapshotRegistry` at `start()`.
//!
//! # Wire parity
//!
//! The registered closure mirrors `actor/typed_projections/mod.rs`'s
//! `signer_state_typed`: returns `Some(TypedProjectionData)` when the slot
//! holds `Some`, `None` (no sidecar entry) when idle. The payload is encoded
//! via [`encode_signer_state`] — same codec as the native path.
//!
//! # Signer registry wiring
//!
//! `register_signer_state_projection` also accepts a callback hook for when
//! the signer registry updates state (e.g. on provider registration). The
//! seam is minimal: on provider-registered events the caller invokes
//! `update_signer_state(slot, model)` to set the slot, which the registered
//! closure then reads on the next tick. The full lifecycle (ready /
//! reconnecting / failed) is a follow-up; the seam exists now.
//!
//! D6 — total: a poisoned slot mutex degrades to `None` (no sidecar entry);
//! the projection simply disappears until the mutex is recovered. No panics.

use std::sync::{Arc, Mutex};

use nmp_core::{
    encode_signer_state, SignerStateModel, TypedProjectionData, SIGNER_STATE_FILE_IDENTIFIER,
    SIGNER_STATE_SCHEMA_ID, SIGNER_STATE_SCHEMA_VERSION,
};

/// Browser-runtime signer-state slot: `Arc<Mutex<Option<SignerStateModel>>>`.
///
/// The outer `Arc` is cloned into the snapshot-projection closure registered
/// at `start()` and held by `BrowserRuntimeHandle::set_signer_state`. The
/// `Mutex<Option<...>>` mirrors the native actor's `SignerStateSlot` design:
/// `None` = no active signer; `Some(model)` = signer is ready or degraded.
pub type BrowserSignerStateSlot = Arc<Mutex<Option<SignerStateModel>>>;

/// Construct a fresh, empty `BrowserSignerStateSlot`.
#[must_use]
pub fn new_signer_state_slot() -> BrowserSignerStateSlot {
    Arc::new(Mutex::new(None))
}

/// Register the typed `"signer_state"` projection closure on `reducer`.
///
/// The closure captures `slot` by clone and is called on each
/// `KernelReducer::make_update_frame` tick. It returns:
/// - `Some(TypedProjectionData)` when the slot holds `Some(model)` (mirrors
///   the native actor's emit-on-Some pattern; sidecar appears in the frame).
/// - `None` when the slot is `None` or the mutex is poisoned (D6: no sidecar
///   entry — the projection silently disappears, same as native `None` state).
///
/// D4: the closure only READS the slot; `set_signer_state` is the sole writer
/// and is only called from `BrowserRuntimeHandle` methods (never from pump).
pub fn register_signer_state_projection(
    reducer: &nmp_core::KernelReducer,
    slot: BrowserSignerStateSlot,
) {
    let file_id = String::from_utf8_lossy(SIGNER_STATE_FILE_IDENTIFIER).into_owned();
    reducer.register_typed_snapshot_projection(SIGNER_STATE_SCHEMA_ID, move || {
        // D6: poisoned slot → None (no sidecar; same as idle state).
        let model = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()?;
        Some(TypedProjectionData {
            key: SIGNER_STATE_SCHEMA_ID.to_string(),
            schema_id: SIGNER_STATE_SCHEMA_ID.to_string(),
            schema_version: SIGNER_STATE_SCHEMA_VERSION,
            file_identifier: file_id.clone(),
            payload: encode_signer_state(&model),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        })
    });
}

/// Write a new signer-state model into the slot.
///
/// The next `KernelReducer::make_update_frame` tick will pick up the new
/// value and emit it in the `"signer_state"` typed sidecar. D4: this is the
/// only writer path (called from `BrowserRuntimeHandle::set_signer_state`).
/// D6: a poisoned slot is silently recovered (`into_inner`).
pub fn update_signer_state(slot: &BrowserSignerStateSlot, model: Option<SignerStateModel>) {
    if let Ok(mut guard) = slot.lock() {
        *guard = model;
    }
    // D6: poisoned slot → silent drop; the state simply stays stale.
}
