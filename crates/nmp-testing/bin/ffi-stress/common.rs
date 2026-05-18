//! Shared helpers used across S3/S4/S5 stress scenarios.
//!
//! P1.a (real ingest path): all event injection now routes through
//! `nmp_app_inject_pre_verified_events`, which calls the same kernel
//! `ingest_timeline_event` path that relay delivery uses.  The old
//! `InjectSyntheticEvents` shortcut (direct HashMap write) is removed.

use crate::ffi::{
    nmp_app_configure, nmp_app_inject_pre_verified_events, nmp_app_inject_signed_events, NmpApp,
};
use std::ffi::CString;
use std::time::Duration;

/// Inject `count` pre-verified kind-1 events into the kernel via the real
/// `ingest_timeline_event` path (test-support only).
///
/// Uses `VerifiedEvent::from_raw_unchecked` internally (D7: capability boundary
/// gated on `cfg(test-support)`; not part of production FFI).  Suitable for
/// S3 (100k events) where Schnorr verification cost would dominate.
///
/// For S4/S5 use `inject_signed_events` (via `nmp_core::testing`) which
/// produces real Schnorr-signed events through `EventBuilder::sign_with_keys`.
pub(crate) fn inject_pre_verified_events(app: *mut NmpApp, prefix: &str, base_ts: u64, count: u32) {
    let prefix_cstr = CString::new(prefix).expect("prefix has no interior nuls");
    nmp_app_inject_pre_verified_events(app, prefix_cstr.as_ptr(), base_ts, count);
}

/// Inject `count` real Schnorr-signed kind-1 events via the full
/// `try_from_raw` verify path.  Use for S4/S5 (small counts; ~10–25 ms).
///
/// For S3 (100k events) use `inject_pre_verified_events` instead.
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
