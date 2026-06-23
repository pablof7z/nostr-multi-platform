//! Tests for typed sidecar projections: `bunker_handshake`, `nip46_onboarding`,
//! and `signer_state` (ADR-0048 D6).
//!
//! These tests prove the projections are driven by REAL transitions through the
//! identity-runtime setter and that the snapshot reflects them correctly.

use std::sync::Arc;

use super::{
    super::*,
    fresh, stub_signer,
};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

// ──────────────────────────────────────────────────────────────────────────
// Typed-sidecar frame proofs (ADR-0037 + ADR-0048 D6)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn frame_carries_bunker_handshake_typed_sidecar_only_when_some() {
    // Full-frame integration proof (ADR-0037): register BOTH the generic and
    // the typed `"bunker_handshake"` projections exactly as the actor does
    // (`run_actor_with_observers`), then decode the SnapshotFrame `make_update`
    // actually emits. The typed sidecar entry must be ABSENT while the slot is
    // idle (mirroring JSON `null`) and PRESENT — decoding back to the same
    // value — once a handshake is in flight.
    let bunker_slot = new_bunker_handshake_slot();
    let id = IdentityRuntime::new(
        Arc::clone(&bunker_slot),
        crate::actor::new_signer_state_slot(),
    );
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let projections = crate::kernel::new_snapshot_projection_slot();
    {
        let typed_slot = Arc::clone(&bunker_slot);
        let mut registry = projections.lock().expect("registry lock");
        registry.register_typed("bunker_handshake", move || {
            crate::actor::typed_projections::bunker_handshake_typed(&typed_slot)
        });
    }
    kernel.set_snapshot_projection_handle(projections);

    // Idle: typed sidecar must NOT carry the key.
    let (_value, typed) = kernel.make_update_typed_for_test(true);
    assert!(
        !typed.iter().any(|t| t.key == "bunker_handshake"),
        "no bunker_handshake typed sidecar while the slot is idle: {typed:?}"
    );

    // In flight: typed sidecar carries the key, decodes back to the live state.
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "connecting".to_string(),
        None,
        Some("dialing wss://r.example".to_string()),
    );
    let (_value, typed) = kernel.make_update_typed_for_test(true);
    let entry = typed
        .iter()
        .find(|t| t.key == "bunker_handshake")
        .expect("bunker_handshake typed sidecar present once a handshake is in flight");
    assert_eq!(entry.file_identifier, "KBHS");
    let decoded = crate::actor::typed_projections::decode_bunker_handshake(&entry.payload)
        .expect("typed sidecar decodes");
    assert_eq!(decoded.stage, "connecting");
    assert_eq!(decoded.message.as_deref(), Some("dialing wss://r.example"));
    assert!(decoded.is_in_flight);
}

#[test]
fn frame_carries_nip46_onboarding_typed_sidecar_always() {
    // Full-frame integration proof (ADR-0037): unlike `bunker_handshake`, the
    // `"nip46_onboarding"` typed sidecar is ALWAYS present (the static
    // signer-app table is emitted even when idle), mirroring the JSON
    // projection's never-`null` contract.
    let bunker_slot = new_bunker_handshake_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let projections = crate::kernel::new_snapshot_projection_slot();
    {
        let typed_slot = Arc::clone(&bunker_slot);
        let mut registry = projections.lock().expect("registry lock");
        registry.register_typed("nip46_onboarding", move || {
            crate::actor::typed_projections::nip46_onboarding_typed(&typed_slot)
        });
    }
    kernel.set_snapshot_projection_handle(projections);

    // Even on an idle slot the typed sidecar is present.
    let (_value, typed) = kernel.make_update_typed_for_test(true);
    let entry = typed
        .iter()
        .find(|t| t.key == "nip46_onboarding")
        .expect("nip46_onboarding typed sidecar present even when idle");
    assert_eq!(entry.file_identifier, "KN46");
    let decoded = crate::actor::typed_projections::decode_nip46_onboarding(&entry.payload)
        .expect("typed sidecar decodes");
    assert!(
        !decoded.signer_apps.is_empty(),
        "static signer-app table is always present"
    );
    assert_eq!(decoded.stage_kind, None);
}

// ──────────────────────────────────────────────────────────────────────────
// ADR-0048 D6: `signer_state` projection tests (generalised from the V-14
// step b `bunker_connection_state` projection).
//
// These tests prove the `"signer_state"` projection is driven by REAL
// transitions through the identity-runtime setter and that the snapshot
// reflects them correctly. No live socket required — the command handlers
// (`bunker_connection_state_changed` for the NIP-46 broker path,
// `nip55_signer_state_changed` for the NIP-55 capability path) are called
// directly.
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn signer_state_projection_reflects_transitions() {
    use crate::actor::commands::{bunker_connection_state_changed, new_signer_state_slot};
    use crate::actor::new_bunker_handshake_slot;

    // Wire up a signer-state slot + identity runtime, register the
    // `"signer_state"` projection closure, bind it onto a kernel.
    let signer_state_slot = new_signer_state_slot();
    let id = IdentityRuntime::new(new_bunker_handshake_slot(), Arc::clone(&signer_state_slot));
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let projections = crate::kernel::new_snapshot_projection_slot();
    {
        let slot = Arc::clone(&signer_state_slot);
        projections
            .lock()
            .expect("registry lock")
            .register_typed("signer_state", move || {
                crate::actor::typed_projections::signer_state_typed(&slot)
            });
    }
    kernel.set_snapshot_projection_handle(projections);

    // 1. Initial state: projection key is absent (no active remote-signer session).
    let (_value, typed) = kernel.make_update_typed_for_test(true);
    assert!(
        !typed.iter().any(|t| t.key == "signer_state"),
        "idle slot must not produce a signer_state typed sidecar entry"
    );

    // 2. Simulate the broker reporting "connected" after handshake completes.
    // ADR-0048 D6: "connected" is mapped to "ready" in the unified SignerStateDto.
    bunker_connection_state_changed(&id, &mut kernel, "connected".to_string(), None);
    let (_v, typed) = kernel.make_update_typed_for_test(true);
    let entry = typed
        .iter()
        .find(|t| t.key == "signer_state")
        .expect("signer_state typed sidecar present after connected");
    let dto = crate::actor::typed_projections::decode_signer_state(&entry.payload)
        .expect("signer_state decodes");
    assert_eq!(dto.state, "ready", "connected transition must surface as 'ready'");
    assert_eq!(
        dto.signer_kind, "nip46",
        "NIP-46 broker path must stamp signer_kind=nip46"
    );
    assert!(dto.is_ready);
    assert!(!dto.is_reconnecting);
    assert!(!dto.is_failed);

    // 3. Simulate a relay flap → "reconnecting".
    bunker_connection_state_changed(
        &id,
        &mut kernel,
        "reconnecting".to_string(),
        Some("connection reset by peer".to_string()),
    );
    let (_v, typed) = kernel.make_update_typed_for_test(true);
    let entry = typed
        .iter()
        .find(|t| t.key == "signer_state")
        .expect("signer_state typed sidecar present after reconnecting");
    let dto = crate::actor::typed_projections::decode_signer_state(&entry.payload)
        .expect("signer_state decodes");
    assert_eq!(dto.state, "reconnecting", "relay flap must project reconnecting");
    assert!(dto.is_reconnecting);
    assert_eq!(dto.reason.as_deref(), Some("connection reset by peer"));

    // 4. Simulate a permanent failure → "failed".
    bunker_connection_state_changed(
        &id,
        &mut kernel,
        "failed".to_string(),
        Some("403 Forbidden".to_string()),
    );
    let (_v, typed) = kernel.make_update_typed_for_test(true);
    let entry = typed
        .iter()
        .find(|t| t.key == "signer_state")
        .expect("signer_state typed sidecar present after failed");
    let dto = crate::actor::typed_projections::decode_signer_state(&entry.payload)
        .expect("signer_state decodes");
    assert_eq!(dto.state, "failed", "permanent failure must project failed");
    assert!(dto.is_failed);
    assert_eq!(dto.reason.as_deref(), Some("403 Forbidden"));
}

#[test]
fn signer_state_slot_reflects_direct_write() {
    // Drive `bunker_connection_state_changed` (the pub command handler) directly
    // to prove the slot writer pre-computes flags correctly without going
    // through the actor loop. Uses the test-accessor to read back the slot.
    use crate::actor::commands::{bunker_connection_state_changed, new_signer_state_slot};
    use crate::actor::new_bunker_handshake_slot;

    let signer_state_slot = new_signer_state_slot();
    let id = IdentityRuntime::new(new_bunker_handshake_slot(), Arc::clone(&signer_state_slot));
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Idle: slot is None.
    assert!(id.signer_state_for_test().is_none());

    // Write "reconnecting" via the command handler.
    bunker_connection_state_changed(
        &id,
        &mut kernel,
        "reconnecting".to_string(),
        Some("timeout".to_string()),
    );
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after reconnecting");
    assert_eq!(dto.state, "reconnecting");
    assert_eq!(dto.signer_kind, "nip46");
    assert!(dto.is_reconnecting);
    assert!(!dto.is_ready);
    assert!(!dto.is_failed);
    assert_eq!(dto.reason.as_deref(), Some("timeout"));

    // Overwrite with "connected" (mapped to "ready" by ADR-0048 D6).
    bunker_connection_state_changed(&id, &mut kernel, "connected".to_string(), None);
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after connected");
    assert!(dto.is_ready, "connected maps to is_ready=true");
    assert!(!dto.is_reconnecting);
    assert!(!dto.is_failed);
    assert!(dto.reason.is_none());

    // Overwrite with "failed".
    bunker_connection_state_changed(
        &id,
        &mut kernel,
        "failed".to_string(),
        Some("403 Forbidden".to_string()),
    );
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after failed");
    assert!(dto.is_failed);
    assert!(!dto.is_ready);
    assert!(!dto.is_reconnecting);
    assert_eq!(dto.reason.as_deref(), Some("403 Forbidden"));
}

#[test]
fn signer_state_slot_reflects_nip55_transitions() {
    // ADR-0048 D6: the NIP-55 capability path writes into the SAME slot via
    // `nip55_signer_state_changed`, stamping `signer_kind = "nip55"` and the
    // NIP-55-specific states (`awaiting_approval` / `unavailable`).
    use crate::actor::commands::{new_signer_state_slot, nip55_signer_state_changed};
    use crate::actor::new_bunker_handshake_slot;

    let signer_state_slot = new_signer_state_slot();
    let id = IdentityRuntime::new(new_bunker_handshake_slot(), Arc::clone(&signer_state_slot));
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Intent round-trip in flight → "awaiting_approval" drives the host's
    // "Waiting for Amber…" inline affordance.
    nip55_signer_state_changed(&id, &mut kernel, "awaiting_approval".to_string(), None);
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after awaiting_approval");
    assert_eq!(dto.signer_kind, "nip55");
    assert_eq!(dto.state, "awaiting_approval");
    assert!(dto.is_awaiting_approval);
    assert!(!dto.is_ready);
    assert!(!dto.is_unavailable);

    // Signer app uninstalled mid-session → "unavailable" prompts re-auth.
    nip55_signer_state_changed(
        &id,
        &mut kernel,
        "unavailable".to_string(),
        Some("signer app not installed".to_string()),
    );
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after unavailable");
    assert!(dto.is_unavailable);
    assert!(!dto.is_awaiting_approval);
    assert_eq!(dto.reason.as_deref(), Some("signer app not installed"));

    // Approval granted → "ready".
    nip55_signer_state_changed(&id, &mut kernel, "ready".to_string(), None);
    let dto = id
        .signer_state_for_test()
        .expect("slot must be Some after ready");
    assert!(dto.is_ready);
    assert_eq!(dto.signer_kind, "nip55");
    assert!(dto.reason.is_none());
}
