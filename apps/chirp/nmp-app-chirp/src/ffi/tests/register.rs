//! Generic register / snapshot / unregister round-trip tests for the
//! Chirp per-app FFI surface.

// ADR-0037: this module imports and exercises the now-`#[deprecated]`
// `nmp_app_chirp_snapshot` (diagnostics-only). Opt the whole test module out of
// the deprecation lint so the `use` import and call sites stay warning-clean.
#![allow(deprecated)]

use std::ffi::CStr;

use nmp_ffi::{nmp_app_free, nmp_app_new};

use super::super::{
    nmp_app_chirp_register, nmp_app_chirp_snapshot, nmp_app_chirp_snapshot_free,
    nmp_app_chirp_unregister,
};

#[test]
fn register_snapshot_unregister_round_trip() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null(), "register returned null");

    // Empty snapshot — no events have arrived.
    let snap = nmp_app_chirp_snapshot(handle);
    assert!(!snap.is_null());
    // SAFETY: snap is a valid C string from our own CString.
    let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
    nmp_app_chirp_snapshot_free(snap);
    // V-80 rung 7: the snapshot is now the OP-centric `RootFeedSnapshot` —
    // `{ "cards": [], "page": … }`. The old `blocks` array is gone.
    assert!(json.contains("\"cards\":[]"), "empty OP feed snapshot, got {json}");
    assert!(!json.contains("\"blocks\""), "RootFeedSnapshot carries no `blocks` key, got {json}");

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn null_handle_paths_are_silent_noops() {
    nmp_app_chirp_unregister(std::ptr::null_mut());
    let snap = nmp_app_chirp_snapshot(std::ptr::null_mut());
    assert!(snap.is_null());
    nmp_app_chirp_snapshot_free(std::ptr::null_mut());
}

#[test]
fn register_with_null_app_returns_null() {
    let handle = nmp_app_chirp_register(std::ptr::null_mut(), std::ptr::null());
    assert!(handle.is_null());
}
