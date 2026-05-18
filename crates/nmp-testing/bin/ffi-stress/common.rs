//! Shared helpers used across S3/S4/S5 stress scenarios.

use crate::ffi::{nmp_app_configure, nmp_app_inject_events, NmpApp};
use std::ffi::CString;
use std::time::Duration;

/// Inject `count` synthetic timeline events into the kernel via the
/// `nmp_app_inject_events` FFI symbol (test-support only).
///
/// Blocks until the inject command has been enqueued (fire-and-forget per
/// bible #3 — the actor processes them asynchronously).  Callers that need
/// events to be visible before the next emit should insert a short sleep or
/// trigger a `nmp_app_configure` call to force an emit tick.
pub(crate) fn inject_events(app: *mut NmpApp, prefix: &str, base_ts: u64, count: u32) {
    let prefix_cstr = CString::new(prefix).expect("prefix has no interior nuls");
    nmp_app_inject_events(app, prefix_cstr.as_ptr(), base_ts, count);
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
