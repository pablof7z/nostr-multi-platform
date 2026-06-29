//! Shared helpers used across ffi-stress scenarios.
//!
//! All event injection uses the real ingest path (VerifiedEvent + EventStore::insert)
//! via `nmp_app_inject_signed_events` (full Schnorr verify via try_from_raw).
//! S3 switched from `inject_pre_verified_events` (from_raw_unchecked) to signed events
//! in T44 round-4 so the signature-verification cost is included in the S3 measurement.

use crate::ffi::{configure_app, nmp_app_inject_signed_events, NmpApp};
use nmp_testing::harness_probe::FrameProbe;
use std::time::Duration;

/// Inject `count` real Schnorr-signed kind-1 events via the full
/// `try_from_raw` verify path.
///
/// Uses `Keys::generate() + EventBuilder::text_note + sign_with_keys`.
/// Schnorr sign cost: ~30-50 µs/event.  For S4/S5 (500/200 events): ~10-25 ms.
/// For S3 (100k events): ~3-8 s; the S3 default settle is 10 s to account for this.
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`.
pub(crate) fn inject_signed_events(app: *mut NmpApp, base_ts: u64, count: u32) {
    nmp_app_inject_signed_events(app, base_ts, count);
}

/// Trigger `configure` to force an emit tick and block — event-driven
/// (Doctrine D8: no sleep/check polling) — until the callback records a new
/// frame, i.e. the actor has processed the pending batch and fired the update
/// callback at least once.
///
/// `frame_count` reads the callback-owned frame tally (under its own lock); the
/// callback fires its [`ProbeSignal`] after recording each frame, waking the
/// `probe`. The wait returns the instant the emit lands instead of always
/// consuming the full settle budget. `deadline_ms` preserves the old fixed
/// settle duration as the upper bound. Returns `true` if a frame advanced
/// before the deadline, `false` on timeout.
///
/// [`ProbeSignal`]: nmp_testing::harness_probe::ProbeSignal
pub(crate) fn configure_and_await_frame(
    app: *mut NmpApp,
    probe: &FrameProbe,
    deadline_ms: u64,
    mut frame_count: impl FnMut() -> usize,
) -> bool {
    let before = frame_count();
    configure_app(app, 500, 12);
    probe.recv_until(Duration::from_millis(deadline_ms), || {
        frame_count() > before
    })
}

/// Block — event-driven (Doctrine D8) — until the callback records a new frame,
/// without calling `configure` first. Used by S4 which manages its own configure
/// schedule: the scenario issues its own `configure_app` call, then calls
/// `await_frame` to block until the emit arrives.
///
/// Returns `true` if a frame advanced before the deadline, `false` on timeout.
pub(crate) fn await_frame(
    probe: &FrameProbe,
    deadline_ms: u64,
    mut frame_count: impl FnMut() -> usize,
) -> bool {
    let before = frame_count();
    probe.recv_until(std::time::Duration::from_millis(deadline_ms), || {
        frame_count() > before
    })
}

/// Extract the `rev` field from a FlatBuffers update frame (typed Tier-3
/// envelope field — PR-B: the generic JSON payload no longer exists).
pub(crate) fn extract_rev(bytes: &[u8]) -> Option<u64> {
    Some(nmp_core::decode_snapshot_envelope(bytes).ok()?.rev)
}

/// Decode an update frame into the typed Tier-3 envelope used by stress gates.
pub(crate) fn snapshot_envelope(bytes: &[u8]) -> Option<nmp_core::SnapshotEnvelope> {
    nmp_core::decode_snapshot_envelope(bytes).ok()
}

/// Return `true` if the non-zero elements of `revs` are strictly increasing.
pub(crate) fn revs_strictly_increasing(revs: &[u64]) -> bool {
    let non_zero: Vec<u64> = revs.iter().copied().filter(|&r| r > 0).collect();
    if non_zero.len() < 2 {
        return true;
    }
    non_zero.windows(2).all(|w| w[1] > w[0])
}

/// Return the `pct`-th percentile of a pre-sorted slice.
pub(crate) fn percentile_u64(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) * pct) / 100;
    sorted[idx]
}
