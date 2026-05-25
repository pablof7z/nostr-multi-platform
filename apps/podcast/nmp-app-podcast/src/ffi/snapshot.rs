//! Snapshot + unregister entry points the host calls against a
//! [`PodcastHandle`] returned by [`super::register::nmp_app_podcast_register`].
//!
//! At M0.A, `nmp_app_podcast_snapshot` returns a stub JSON payload:
//! `{"running":true,"rev":0,"schema_version":1}`.
//! The schema_version field lets the Swift decoder detect when a real snapshot
//! payload lands in a later milestone.

use std::ffi::{c_char, CString};

use super::handle::PodcastHandle;

/// Serialize the current Podcast snapshot into a JSON C string.
///
/// At M0.A this returns the stub payload
/// `{"running":true,"rev":0,"schema_version":1}`.
/// Returns null on a null handle or a `CString` nul-byte conflict (which
/// cannot occur with the stub payload). The returned pointer is owned by the
/// caller; pass it to [`nmp_app_podcast_snapshot_free`] when done.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_snapshot(handle: *mut PodcastHandle) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    let payload = r#"{"running":true,"rev":0,"schema_version":1}"#;
    match CString::new(payload) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a snapshot string previously returned by
/// [`nmp_app_podcast_snapshot`]. Null pointer is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_snapshot_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ptr` came from `CString::into_raw` in
    // `nmp_app_podcast_snapshot` and has not been freed.
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

/// Drop the handle and free all associated resources. Idempotent: null pointer
/// is a silent no-op. The handle MUST NOT be used after this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_podcast_unregister(handle: *mut PodcastHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from `nmp_app_podcast_register`
    // and has not already been freed. Drop releases the box.
    let _boxed = unsafe { Box::from_raw(handle) };
    // No event observer to unregister at M0.A — the handle holds no
    // projection or observer_id. Future milestones add unregister calls here.
}
