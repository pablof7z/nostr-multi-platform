//! Snapshot + unregister entry points the host calls against a
//! [`ChirpHandle`] returned by [`super::register::nmp_app_chirp_register`].

use std::ffi::{c_char, CString};

use super::handle::ChirpHandle;

/// Serialize the current `ChirpTimelineSnapshot` into a JSON C string.
///
/// **Deprecated**: this is a diagnostics-only export as of ADR-0035. Runtime
/// hosts should consume the typed `nmp.feed.home` projection from the update
/// stream instead. This function remains for diagnostics/REPL use only and
/// will be removed in a future release.
///
/// Returns null on any failure (null handle, JSON encode error, `CString`
/// nul-byte conflict). The returned pointer is owned by the caller; pass it
/// to [`nmp_app_chirp_snapshot_free`] when done.
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
    snapshot_to_c_string(&handle.projection.snapshot())
}

fn snapshot_to_c_string(snapshot: &nmp_nip01::ModularTimelineSnapshot) -> *mut c_char {
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

/// Drop the projection's observer registration and free the handle.
/// Idempotent: null pointer is a silent no-op. The handle MUST NOT be used
/// after this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_unregister(handle: *mut ChirpHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from `nmp_app_chirp_register`
    // and has not already been freed.
    let boxed = unsafe { Box::from_raw(handle) };
    if !boxed.app.is_null() {
        // SAFETY: same `app` validity rule as `nmp_app_chirp_register` — the
        // caller is responsible for the `nmp_app_free` ordering invariant.
        let app_ref = unsafe { &*boxed.app };
        app_ref.unregister_event_observer(boxed.observer_id);
    }
    // boxed dropped here — projection's last Arc released only if no other
    // strong refs exist (none should once the observer is unregistered).
}
