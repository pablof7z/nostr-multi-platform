//! V-62 / #1651: `init_error` surfaced in the `MarmotProjection` snapshot.

use nostr::Keys;

use super::fixtures::in_memory_service;

/// V-62 / #1651: a `MarmotProjection` built with
/// `Some(MarmotInitError::KeyringUnavailable)` surfaces that reason in every
/// snapshot (replaces the former `keyring_unavailable` bool assertion).
#[test]
fn keyring_unavailable_is_surfaced_in_snapshot() {
    use crate::projection::payload::MarmotInitError;
    use crate::projection::state::MarmotProjection;

    let service = in_memory_service(Keys::generate());
    // Host registration initialized Marmot with a degraded credential store.
    let proj = MarmotProjection::new(service, Some(MarmotInitError::KeyringUnavailable));
    let snap = proj.snapshot(0);
    assert_eq!(
        snap.init_error,
        Some(MarmotInitError::KeyringUnavailable),
        "init_error must be KeyringUnavailable with the mock store"
    );
}

/// V-62 / #1651: a `MarmotProjection` built with `None` carries no init error —
/// the real Keychain is in use, no warning needed.
#[test]
fn keyring_available_not_flagged_in_snapshot() {
    use crate::projection::state::MarmotProjection;

    let service = in_memory_service(Keys::generate());
    // Host registration initialized Marmot with a healthy credential store.
    let proj = MarmotProjection::new(service, None);
    let snap = proj.snapshot(0);
    assert_eq!(
        snap.init_error, None,
        "init_error must be None when the real Keychain is in use"
    );
}
