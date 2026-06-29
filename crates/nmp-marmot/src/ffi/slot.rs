//! Marmot push-projection slot state + the SOLE registration path for the two
//! `nmp.marmot.{snapshot,messages}` keys (ADR-0039, #1651).
//!
//! Split out of `ffi.rs` to keep that file under the 500-LOC ceiling. The slot
//! is the single state the two push-projection closures read on every tick; it
//! serves the live (`Ready`), init-failed (`InitFailed`), and signed-out
//! (`Cleared`) outcomes through one closure shape so a later `register_with_keys`
//! cleanly REPLACES the prior state by key.

use std::sync::{Arc, Mutex};

use nmp_native_runtime::NmpApp;

use super::DEFAULT_MESSAGE_PAGE;
use crate::projection::payload::{MarmotInitError, MarmotSnapshot};
use crate::projection::state::MarmotProjection;

/// State the two Marmot push-projection closures read on every tick (ADR-0039,
/// #1651). One slot serves all three registration outcomes through a single
/// code path so a later `register_with_keys` (account switch / the user
/// recovered the keyring key) REPLACES the prior state cleanly:
///
/// * [`MarmotSlotState::Cleared`] — signed out (`nmp_marmot_unregister`). The
///   snapshot/messages closures emit nothing (`None`) until a new account
///   registers.
/// * [`MarmotSlotState::Ready`] — a live `MarmotService` was registered; the
///   snapshot closure reads `proj.snapshot(now)` and messages reads the
///   per-group map.
/// * [`MarmotSlotState::InitFailed`] — `MarmotService::new` returned `Err`
///   (#1651: encrypted MLS DB exists but its keyring key was lost, or the
///   keyring was unavailable). There is NO `MarmotProjection` here (it wraps a
///   `MarmotService` we do not have), so the snapshot closure builds a degraded
///   `MarmotSnapshot` (`init_error: Some(..)`, `is_registered: false`) directly
///   from the carried reason. This is the kernel-owned state that surfaces the
///   init failure to the shells (it used to be stderr-only).
pub enum MarmotSlotState {
    /// Signed out — closures emit nothing.
    Cleared,
    /// Live service — closures read the projection.
    Ready(Arc<MarmotProjection>),
    /// Service init failed — closures emit a degraded snapshot carrying this reason.
    InitFailed(MarmotInitError),
}

/// Shared slot the two Marmot push-projection closures capture and read on every
/// tick. `Send + Sync` because it is an `Arc<Mutex<_>>`; the closures capture it
/// by `Arc::clone`, run on the actor thread, and read under the lock.
pub type MarmotProjectionSlot = Arc<Mutex<MarmotSlotState>>;

/// Distinctive fragment of mdk's `KeyringEntryMissingForExistingDatabase`
/// message ("Database exists … but no encryption key found in keyring …") —
/// the one service-init failure that means the encryption key is permanently
/// lost. `MarmotError` flattens the mdk error to an opaque string, so this
/// message fragment is the only available discriminator at this layer.
const DB_KEY_LOST_SIGNATURE: &str = "no encryption key found in keyring";

/// Classify a `MarmotService::new` error (stringified) into a typed
/// [`MarmotInitError`]. ONLY the lost-encryption-key case maps to
/// [`MarmotInitError::DbKeyLost`] (the permanent, "data unrecoverable"
/// condition #1651 is about); every other init failure — disk full, unwritable
/// path, unencrypted-DB-with-encryption, etc. — maps to
/// [`MarmotInitError::Other`] so shells do NOT render the unrecoverable-data
/// copy for a benign/transient error.
pub(super) fn classify_service_init_error(detail: &str) -> MarmotInitError {
    if detail.to_ascii_lowercase().contains(DB_KEY_LOST_SIGNATURE) {
        MarmotInitError::DbKeyLost {
            detail: detail.to_string(),
        }
    } else {
        MarmotInitError::Other {
            detail: detail.to_string(),
        }
    }
}

/// Build the degraded `MarmotSnapshot` the `nmp.marmot.snapshot` closure emits
/// while the slot is [`MarmotSlotState::InitFailed`] — everything default /
/// empty except the carried `init_error` (`is_registered: false`). #1651.
fn init_failed_snapshot(init_error: &MarmotInitError) -> MarmotSnapshot {
    MarmotSnapshot {
        init_error: Some(init_error.clone()),
        ..MarmotSnapshot::empty()
    }
}

/// Register the two Marmot push projections (`nmp.marmot.snapshot` /
/// `nmp.marmot.messages`) against `app_ref`, both reading the shared `slot`
/// (#1651). This is the SOLE registration path for those two keys — it serves
/// `Ready`, `InitFailed`, and `Cleared` slot states uniformly, so a later
/// `register_with_keys` (account switch / keyring recovered) re-registers the
/// SAME keys with a fresh slot and REPLACES the prior closures (last-writer-wins
/// by key — see `register_typed_snapshot_projection`). Whether the service init
/// succeeded only decides what the closure finds in the slot, not whether the
/// projection is registered — the failure is now always visible kernel state.
pub(super) fn register_marmot_snapshot_projections(app_ref: &NmpApp, slot: &MarmotProjectionSlot) {
    // **`nmp.marmot.snapshot`**: group list / membership / key-package / pending
    // welcomes when `Ready`; a degraded init-error snapshot when `InitFailed`;
    // nothing when `Cleared`. Cheap on the `Ready` path: one lock + MDK reads.
    {
        let typed_snap_slot = Arc::clone(slot);
        app_ref.register_typed_snapshot_projection_with_time(
            "nmp.marmot.snapshot",
            move |now_secs| {
                let guard = typed_snap_slot.lock().ok()?;
                let snapshot = match &*guard {
                    MarmotSlotState::Cleared => return None,
                    MarmotSlotState::Ready(proj) => proj.snapshot(now_secs),
                    MarmotSlotState::InitFailed(init_error) => init_failed_snapshot(init_error),
                };
                Some(crate::wire::snapshot_fb::typed_projection(&snapshot))
            },
        );
    }

    // **`nmp.marmot.messages`**: per-group decrypted-message map; only the
    // `Ready` slot has a service to read, so `InitFailed` / `Cleared` emit nothing.
    {
        let typed_msgs_slot = Arc::clone(slot);
        app_ref.register_typed_snapshot_projection("nmp.marmot.messages", move || {
            let guard = typed_msgs_slot.lock().ok()?;
            let MarmotSlotState::Ready(proj) = &*guard else {
                return None;
            };
            Some(crate::wire::messages_fb::typed_projection(
                &proj.messages_all_groups(DEFAULT_MESSAGE_PAGE),
            ))
        });
    }
}
