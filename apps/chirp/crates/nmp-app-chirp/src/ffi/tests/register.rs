//! Generic register / unregister round-trip tests for the
//! Chirp per-app FFI surface.

use nmp_ffi::{nmp_app_free, nmp_app_new};

use super::super::{nmp_app_chirp_register, nmp_app_chirp_unregister};

#[test]
fn register_unregister_round_trip() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null(), "register returned null");

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn null_handle_unregister_is_silent_noop() {
    nmp_app_chirp_unregister(std::ptr::null_mut());
}

#[test]
fn register_with_null_app_returns_null() {
    let handle = nmp_app_chirp_register(std::ptr::null_mut(), std::ptr::null());
    assert!(handle.is_null());
}
