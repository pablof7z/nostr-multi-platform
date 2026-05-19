//! Shared helpers used across S3/S4/S5 stress scenarios.
//!
//! All event injection uses the real ingest path (VerifiedEvent + EventStore::insert)
//! via `nmp_app_inject_signed_events` (full Schnorr verify via try_from_raw).
//! S3 switched from `inject_pre_verified_events` (from_raw_unchecked) to signed events
//! in T44 round-4 so the signature-verification cost is included in the S3 measurement.

use crate::ffi::{nmp_app_configure, nmp_app_inject_signed_events, NmpApp};
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

/// Trigger `configure` to force an emit tick and wait `settle_ms` for the
/// actor to process the event batch and fire the update callback.
pub(crate) fn configure_and_settle(app: *mut NmpApp, settle_ms: u64) {
    nmp_app_configure(app, 0, 500, 12);
    std::thread::sleep(Duration::from_millis(settle_ms));
}

/// Extract the `"rev":N` field from a JSON byte slice without a full parse.
pub(crate) fn extract_rev(bytes: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(bytes).ok()?;
    let key = "\"rev\":";
    let pos = s.find(key)?;
    let rest = &s[pos + key.len()..];
    let end = rest.find([',', '}', ' ', '\n']).unwrap_or(rest.len());
    rest[..end].trim().parse::<u64>().ok()
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
