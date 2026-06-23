//! Built-in actor-owned snapshot projections.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use super::commands::{BunkerHandshakeSlot, SignerStateSlot};

/// Register actor-owned built-ins that are not kernel snapshot fields.
///
/// These projections are installed at the actor wiring site so FFI and
/// non-FFI actor consumers receive the same remote-signer read models.
#[cfg(feature = "native")]
pub(super) fn register_builtin_projections(
    snapshot_projections: &crate::kernel::SnapshotProjectionSlot,
    bunker_handshake: &BunkerHandshakeSlot,
    signer_state: &SignerStateSlot,
) {
    register_bunker_handshake(snapshot_projections, bunker_handshake);
    register_nip46_onboarding(snapshot_projections, bunker_handshake);
    register_signer_state(snapshot_projections, signer_state);
}

#[cfg(feature = "native")]
fn register_bunker_handshake(
    snapshot_projections: &crate::kernel::SnapshotProjectionSlot,
    bunker_handshake: &BunkerHandshakeSlot,
) {
    let typed_slot = Arc::clone(bunker_handshake);
    if let Ok(mut registry) = snapshot_projections.lock() {
        registry.register_typed("bunker_handshake", move || {
            super::typed_projections::bunker_handshake_typed(&typed_slot)
        });
    }
}

#[cfg(feature = "native")]
fn register_nip46_onboarding(
    snapshot_projections: &crate::kernel::SnapshotProjectionSlot,
    bunker_handshake: &BunkerHandshakeSlot,
) {
    let typed_slot = Arc::clone(bunker_handshake);
    let (
        nip46_onboarding_incremental_apply,
        nip46_onboarding_frame_session_id,
        nip46_onboarding_frame_snapshot_epoch,
    ) = if let Ok(reg) = snapshot_projections.lock() {
        let cap = reg.incremental_apply_handle();
        let (sid, epoch) = reg.frame_identity_handles();
        (cap, sid, epoch)
    } else {
        use std::sync::atomic::AtomicU64;
        (
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        )
    };
    let nip46_onboarding_emission_state = Arc::new(Mutex::new(
        crate::projection_emission::TypedProjectionEmissionState::new(
            nip46_onboarding_incremental_apply,
        ),
    ));
    if let Ok(mut registry) = snapshot_projections.lock() {
        let emission_state = Arc::clone(&nip46_onboarding_emission_state);
        let frame_session_id = Arc::clone(&nip46_onboarding_frame_session_id);
        let frame_snapshot_epoch = Arc::clone(&nip46_onboarding_frame_snapshot_epoch);
        registry.register_typed("nip46_onboarding", move || {
            let typed_data = super::typed_projections::nip46_onboarding_typed(&typed_slot)?;
            let identity = crate::projection_emission::FrameIdentity {
                session_id: frame_session_id.load(Ordering::Acquire),
                snapshot_epoch: frame_snapshot_epoch.load(Ordering::Acquire),
            };
            let Ok(mut state) = emission_state.lock() else {
                return Some(typed_data);
            };
            let emit_decision = state.should_emit(typed_data.payload.clone(), identity);
            drop(state);
            match emit_decision {
                None => None,
                Some((payload, projection_rev)) => {
                    Some(crate::update_envelope::TypedProjectionData {
                        payload,
                        projection_rev,
                        ..typed_data
                    })
                }
            }
        });
    }
}

#[cfg(feature = "native")]
fn register_signer_state(
    snapshot_projections: &crate::kernel::SnapshotProjectionSlot,
    signer_state: &SignerStateSlot,
) {
    let typed_slot = Arc::clone(signer_state);
    if let Ok(mut registry) = snapshot_projections.lock() {
        registry.register_typed("signer_state", move || {
            super::typed_projections::signer_state_typed(&typed_slot)
        });
    }
}
