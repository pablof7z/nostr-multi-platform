//! Snapshot + unregister entry points the host calls against a
//! [`ChirpHandle`] returned by [`super::register::nmp_app_chirp_register`].

use std::ffi::{c_char, CStr, CString};

use nmp_nip01::{TimelineWindowRequest, DEFAULT_TIMELINE_WINDOW_LIMIT, MAX_TIMELINE_WINDOW_LIMIT};

use super::handle::ChirpHandle;

/// Serialize the current `ChirpTimelineSnapshot` into a JSON C string.
/// Returns null on any failure (null handle, JSON encode error, `CString`
/// nul-byte conflict). The returned pointer is owned by the caller; pass it
/// to [`nmp_app_chirp_snapshot_free`] when done.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_snapshot(handle: *mut ChirpHandle) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `handle` is a valid pointer returned by
    // `nmp_app_chirp_register` and not yet freed.
    let handle = unsafe { &*handle };
    snapshot_to_c_string(&handle.projection.snapshot())
}

/// Serialize a bounded timeline window into a JSON C string. `request_json`
/// may be null, in which case Rust returns the default newest window.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_snapshot_window(
    handle: *mut ChirpHandle,
    request_json: *const c_char,
) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    let request = if request_json.is_null() {
        TimelineWindowRequest::default()
    } else {
        let Ok(text) = (unsafe { CStr::from_ptr(request_json) }).to_str() else {
            return std::ptr::null_mut();
        };
        let Ok(request) = serde_json::from_str::<TimelineWindowRequest>(text) else {
            return std::ptr::null_mut();
        };
        request
    };
    // SAFETY: caller guarantees `handle` is a valid pointer returned by
    // `nmp_app_chirp_register` and not yet freed.
    let handle = unsafe { &*handle };
    snapshot_to_c_string(&handle.projection.snapshot_window(request))
}

/// Rust-owned default modular timeline window size. Exported so thin
/// shells do not mirror protocol/app constants.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_default_window_limit() -> u32 {
    DEFAULT_TIMELINE_WINDOW_LIMIT as u32
}

/// Rust-owned modular timeline window cap.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_max_window_limit() -> u32 {
    MAX_TIMELINE_WINDOW_LIMIT as u32
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
