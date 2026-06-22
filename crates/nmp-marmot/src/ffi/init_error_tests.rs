//! #1651 — the Marmot service-init failure is surfaced as kernel-owned typed
//! state, and a later successful registration REPLACES the degraded state.
//!
//! These tests exercise the SOLE snapshot-projection registration path
//! ([`super::register_marmot_snapshot_projections`]) directly against a real
//! `NmpApp`, so they do not need an on-disk keyring + SQLite file to force
//! `MarmotService::new` to `Err` — they drive the `InitFailed` / `Ready` slot
//! states the production `register_with_keys` would install.

use std::sync::{Arc, Mutex};

use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::Keys;

use super::{register_marmot_snapshot_projections, MarmotProjectionSlot, MarmotSlotState};
use crate::projection::payload::MarmotInitError;
use crate::projection::state::MarmotProjection;
use crate::service::MarmotService;
use crate::wire::snapshot_fb::{decode_marmot_snapshot, PROJECTION_KEY};

fn in_memory(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

/// Run every registered typed snapshot projection against `app`, find the
/// `nmp.marmot.snapshot` entry, and decode it back to a `MarmotSnapshot`.
/// `None` when the snapshot closure emitted nothing this tick (e.g. `Cleared`).
fn decoded_snapshot(
    app: *mut nmp_ffi::NmpApp,
) -> Option<crate::projection::payload::MarmotSnapshot> {
    // SAFETY: `app` is a live pointer from `nmp_app_new`, freed by the caller.
    let app_ref = unsafe { &*app };
    let entries = app_ref.run_typed_snapshot_projections();
    let entry = entries.into_iter().find(|e| e.key == PROJECTION_KEY)?;
    Some(decode_marmot_snapshot(&entry.payload).expect("marmot snapshot sidecar must decode"))
}

/// THE KEY TEST (#1651): registering the `InitFailed` snapshot then a
/// successful (`Ready`) registration under the SAME projection key REPLACES the
/// degraded state, so the snapshot no longer reports an init error.
///
/// RED evidence: if the recovery leg is registered under a DIFFERENT key (or
/// skipped), the original `InitFailed` closure stays live under
/// `nmp.marmot.snapshot` and the final `assert_eq!(init_error, None)` fails —
/// proving the test actually exercises replacement-by-key, not two independent
/// registrations. With the real same-key path it goes green.
#[test]
fn degraded_init_error_is_replaced_by_a_successful_registration() {
    let app = nmp_ffi::nmp_app_new();

    // ── Step 1: service init FAILED (e.g. encrypted MLS DB exists but its
    // keyring key was lost). Production `register_with_keys` installs an
    // `InitFailed` slot and registers the snapshot projection from it, then
    // returns a null handle. The failure is now kernel-owned state.
    let detail = "Database exists but no encryption key found in keyring".to_string();
    let degraded_slot: MarmotProjectionSlot = Arc::new(Mutex::new(MarmotSlotState::InitFailed(
        MarmotInitError::DbKeyLost {
            detail: detail.clone(),
        },
    )));
    register_marmot_snapshot_projections(unsafe { &*app }, &degraded_slot);

    let snap = decoded_snapshot(app).expect("InitFailed must emit a degraded snapshot");
    assert_eq!(
        snap.init_error,
        Some(MarmotInitError::DbKeyLost { detail }),
        "the degraded snapshot must carry the DbKeyLost reason: {snap:?}"
    );
    assert!(
        !snap.is_registered,
        "the degraded snapshot must report is_registered = false: {snap:?}"
    );
    assert!(
        snap.groups.is_empty(),
        "the degraded snapshot carries no group state: {snap:?}"
    );

    // ── Step 2: the user recovered the keyring key — a later
    // `register_with_keys` builds a live service and installs a fresh `Ready`
    // slot, re-registering the SAME `nmp.marmot.snapshot` key. Last-writer-wins
    // by key REPLACES the degraded closure cleanly.
    let proj = Arc::new(MarmotProjection::new(in_memory(Keys::generate()), None));
    let ready_slot: MarmotProjectionSlot =
        Arc::new(Mutex::new(MarmotSlotState::Ready(Arc::clone(&proj))));
    register_marmot_snapshot_projections(unsafe { &*app }, &ready_slot);

    let snap = decoded_snapshot(app).expect("Ready must emit a snapshot");
    assert_eq!(
        snap.init_error, None,
        "after recovery the snapshot must report NO init error: {snap:?}"
    );
    assert!(
        snap.is_registered,
        "the recovered snapshot reports a registered identity: {snap:?}"
    );

    nmp_ffi::nmp_app_free(app);
}

/// A `Cleared` slot (sign-out) emits nothing for the snapshot key — neither a
/// degraded nor a healthy snapshot lingers after `nmp_marmot_unregister`.
#[test]
fn cleared_slot_emits_no_snapshot() {
    let app = nmp_ffi::nmp_app_new();
    let slot: MarmotProjectionSlot = Arc::new(Mutex::new(MarmotSlotState::Cleared));
    register_marmot_snapshot_projections(unsafe { &*app }, &slot);
    assert!(
        decoded_snapshot(app).is_none(),
        "a Cleared slot must not emit a marmot snapshot"
    );
    nmp_ffi::nmp_app_free(app);
}

/// `MarmotService::new` failures are classified by message: ONLY the lost-key
/// case (mdk `KeyringEntryMissingForExistingDatabase`) is `DbKeyLost` (the
/// permanent "data unrecoverable" condition); every other init failure is
/// `Other` so the shell shows neutral copy, not the unrecoverable-data banner.
#[test]
fn service_init_error_is_classified_db_key_lost_only_for_lost_key() {
    use super::slot::classify_service_init_error;

    // The real mdk message for an existing encrypted DB whose keyring entry is
    // gone — must classify as the unrecoverable DbKeyLost.
    let lost = "mdk error: Database exists at '/x/marmot.sqlite' but no encryption \
                key found in keyring (service='nmp.chirp.marmot', key='marmot-mls-db-key').";
    assert!(
        matches!(
            classify_service_init_error(lost),
            MarmotInitError::DbKeyLost { .. }
        ),
        "lost-keyring-key message must classify as DbKeyLost"
    );

    // Benign / transient failures must NOT be mislabeled as unrecoverable.
    for benign in [
        "mdk error: disk I/O error: No space left on device",
        "mdk error: unable to open database file: permission denied",
        "mdk error: unencrypted database opened with encryption requested",
    ] {
        assert!(
            matches!(
                classify_service_init_error(benign),
                MarmotInitError::Other { .. }
            ),
            "benign init failure must classify as Other, not DbKeyLost: {benign}"
        );
    }
}
