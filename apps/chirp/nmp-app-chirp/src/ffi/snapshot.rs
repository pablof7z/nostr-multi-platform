//! Snapshot + unregister entry points the host calls against a
//! [`ChirpHandle`] returned by [`super::register::nmp_app_chirp_register`].

use std::ffi::{c_char, CString};

use super::handle::ChirpHandle;

/// Serialize the current `ChirpTimelineSnapshot` into a JSON C string.
///
/// **Deprecated**: this is a diagnostics-only export as of ADR-0037. Runtime
/// hosts should consume the typed `nmp.feed.home` projection from the update
/// stream instead. This function remains for diagnostics/REPL use only and
/// will be removed in a future release.
///
/// Returns null on any failure (null handle, JSON encode error, `CString`
/// nul-byte conflict). The returned pointer is owned by the caller; pass it
/// to [`nmp_app_chirp_snapshot_free`] when done.
///
/// V-80 rung 7 — `ChirpTimelineSnapshot` is now the OP-centric
/// `RootFeedSnapshot` (`{ cards, page, metrics }`), produced by the engine's
/// default-window snapshot. This is the same shape served on the actor
/// `projections["nmp.feed.home"]` path (which the TUI + iOS actually consume);
/// the direct-handle path stays available for parity and tests.
#[deprecated(
    since = "0.1.0",
    note = "Diagnostics only — use the typed nmp.feed.home projection from the update stream"
)]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref, deprecated)]
pub extern "C" fn nmp_app_chirp_snapshot(handle: *mut ChirpHandle) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `handle` is a valid pointer returned by
    // `nmp_app_chirp_register` and not yet freed.
    let handle = unsafe { &*handle };
    let snapshot = handle
        .engine
        .snapshot(&nmp_feed::FeedRequest::default());
    snapshot_to_c_string(&snapshot)
}

fn snapshot_to_c_string(snapshot: &crate::ChirpTimelineSnapshot) -> *mut c_char {
    let Ok(payload) = serde_json::to_string(&snapshot) else {
        return std::ptr::null_mut();
    };
    let Ok(cstr) = CString::new(payload) else {
        return std::ptr::null_mut();
    };
    cstr.into_raw()
}

/// Free a snapshot string previously returned by
/// [`nmp_app_chirp_snapshot`]. Null pointer is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_snapshot_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ptr` came from `CString::into_raw` in
    // `nmp_app_chirp_snapshot` and has not been freed.
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

/// Free the handle.
/// Idempotent: null pointer is a silent no-op. The handle MUST NOT be used
/// after this call.
///
/// V-80 rung 7 — the OP-feed engine + `ActiveFollowSet` observers are
/// registered by `nmp_app_template::register_op_feed_defaults` through the
/// kernel's standard observer registry, NOT through a single swappable slot
/// this handle owns. There is no per-handle `observer_id` to revoke here; the
/// observers live for the life of the `NmpApp` and are torn down by
/// `nmp_app_free` (the actor `join()` fences any in-flight callback — see the
/// `ChirpHandle` `unsafe impl` rationale). Dropping the boxed handle releases
/// this crate's `Arc` clones of the engine and follow set; the kernel keeps
/// its own clones until `nmp_app_free`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_unregister(handle: *mut ChirpHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from `nmp_app_chirp_register`
    // and has not already been freed. Reclaim the box and drop it — releasing
    // this crate's `Arc` clones of the engine + follow set.
    let _boxed = unsafe { Box::from_raw(handle) };
}
