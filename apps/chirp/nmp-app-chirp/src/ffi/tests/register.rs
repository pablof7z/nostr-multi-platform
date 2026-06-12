//! Generic register / unregister round-trip tests for the
//! Chirp per-app FFI surface.
//!
//! V-73 (D6): `nmp_app_chirp_register` must return
//! `NmpRegisterStatus::InvalidViewerPubkey` when a non-null but invalid
//! hex pubkey is supplied, rather than silently falling back to the
//! default empty pubkey identity.

use std::ffi::CString;

use nmp_ffi::{nmp_app_free, nmp_app_new};

use super::super::{nmp_app_chirp_register, nmp_app_chirp_unregister, ChirpHandle, NmpRegisterStatus};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Call `nmp_app_chirp_register` and return `(status_u32, handle)`.
fn reg(
    app: *mut nmp_ffi::NmpApp,
    viewer: *const std::ffi::c_char,
) -> (u32, *mut ChirpHandle) {
    let mut handle: *mut ChirpHandle = std::ptr::null_mut();
    let status = nmp_app_chirp_register(app, viewer, &mut handle);
    (status, handle)
}

// ── existing round-trip behaviour (must still pass) ──────────────────────────

#[test]
fn register_unregister_round_trip() {
    let app = nmp_app_new();
    let (status, handle) = reg(app, std::ptr::null());
    assert_eq!(
        status,
        NmpRegisterStatus::Ok as u32,
        "null viewer_pubkey must succeed"
    );
    assert!(!handle.is_null(), "register returned null handle on Ok");

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn null_handle_unregister_is_silent_noop() {
    nmp_app_chirp_unregister(std::ptr::null_mut());
}

#[test]
fn register_with_null_app_returns_null_handle() {
    let (status, handle) = reg(std::ptr::null_mut(), std::ptr::null());
    assert_eq!(status, NmpRegisterStatus::NullApp as u32);
    assert!(handle.is_null());
}
// ── Finding 1 (D6): null handle_out must return NullApp without crashing ─────

/// Passing a null `handle_out` is a programmer-error contract violation.
/// The function must return `NmpRegisterStatus::NullApp` without writing
/// through the null pointer (which would be a segfault) or leaking the
/// `Box::into_raw` handle allocation.
#[test]
fn null_handle_out_returns_null_app_without_crash() {
    let app = nmp_app_new();
    // Pass null as handle_out — must not segfault, must return NullApp.
    let status = nmp_app_chirp_register(app, std::ptr::null(), std::ptr::null_mut());
    assert_eq!(
        status,
        NmpRegisterStatus::NullApp as u32,
        "null handle_out must return NullApp (programmer-error contract violation)"
    );
    // app was not consumed by a handle, so free it directly.
    nmp_app_free(app);
}

// ── V-73: non-null invalid viewer_pubkey must be rejected ────────────────────

/// A null viewer_pubkey means "no viewer set" — this is explicitly permitted.
/// The register must succeed and return a non-null handle.
#[test]
fn null_viewer_pubkey_is_permitted() {
    let app = nmp_app_new();
    let (status, handle) = reg(app, std::ptr::null());
    assert_eq!(status, NmpRegisterStatus::Ok as u32);
    assert!(!handle.is_null());
    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// A non-null viewer_pubkey that is not 64 lowercase hex chars must be
/// rejected with `InvalidViewerPubkey`. The handle_out must be left as
/// null (D6: no partial state).
#[test]
fn malformed_hex_viewer_pubkey_returns_invalid_status() {
    let bad_pubkey = CString::new("not-a-pubkey").unwrap();
    let app = nmp_app_new();
    let (status, handle) = reg(app, bad_pubkey.as_ptr());
    assert_eq!(
        status,
        NmpRegisterStatus::InvalidViewerPubkey as u32,
        "non-hex viewer_pubkey must produce InvalidViewerPubkey"
    );
    assert!(
        handle.is_null(),
        "handle_out must remain null on InvalidViewerPubkey"
    );
    nmp_app_free(app);
}

/// A pubkey that is hex but only 32 chars (too short — must be 64) must
/// also be rejected.
#[test]
fn too_short_hex_viewer_pubkey_returns_invalid_status() {
    let short_pubkey = CString::new("deadbeef".repeat(4)).unwrap(); // 32 hex chars
    let app = nmp_app_new();
    let (status, handle) = reg(app, short_pubkey.as_ptr());
    assert_eq!(status, NmpRegisterStatus::InvalidViewerPubkey as u32);
    assert!(handle.is_null());
    nmp_app_free(app);
}

/// A well-formed 64-char lowercase hex pubkey must succeed and return a
/// non-null handle.
#[test]
fn valid_hex_viewer_pubkey_succeeds() {
    // 64 lowercase hex chars — a syntactically valid pubkey (value need not
    // resolve to a live Nostr identity for this test).
    let valid_pubkey =
        CString::new("deadbeefcafe0123456789abcdef0123456789abcdef0123456789abcdef0123")
            .unwrap();
    let app = nmp_app_new();
    let (status, handle) = reg(app, valid_pubkey.as_ptr());
    assert_eq!(
        status,
        NmpRegisterStatus::Ok as u32,
        "valid hex viewer_pubkey must succeed"
    );
    assert!(
        !handle.is_null(),
        "handle must be non-null on success with valid viewer_pubkey"
    );
    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// A 64-char string of uppercase hex is also accepted by `is_hex_pubkey`
/// (the helper is case-agnostic). Verify it succeeds so we document the
/// behaviour boundary clearly.
#[test]
fn uppercase_hex_viewer_pubkey_is_accepted() {
    let upper_pubkey =
        CString::new("DEADBEEFCAFE0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123")
            .unwrap();
    let app = nmp_app_new();
    let (status, handle) = reg(app, upper_pubkey.as_ptr());
    // `is_hex_pubkey` accepts both cases — behaviour documented here, not
    // a contract change.
    assert_eq!(status, NmpRegisterStatus::Ok as u32);
    assert!(!handle.is_null());
    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
