//! Tests for the pre-built `(namespace, body_json)` byte doorway.

use std::ffi::{CStr, CString};

use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_free_string};

use super::super::nmp_app_chirp_dispatch_action_bytes;
use super::helpers::register_app;

/// Read + free a `*mut c_char` dispatch envelope into a parsed JSON value.
fn read_dispatch_envelope(ptr: *mut std::ffi::c_char) -> serde_json::Value {
    assert!(!ptr.is_null(), "dispatch envelope must return a JSON string");
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    nmp_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}

/// The direct namespace+body doorway dispatches a known direct action
/// (`nmp.follow`) and echoes a non-empty correlation id — proving the
/// host-registered module + executor are wired and no JSON crosses to the kernel.
#[test]
fn dispatch_action_bytes_direct_namespace_returns_correlation_id() {
    let app = nmp_app_new();
    let handle = register_app(app);

    let namespace = CString::new("nmp.follow").unwrap();
    let body = CString::new(r#"{"pubkey":"deadbeef"}"#).unwrap();
    let value = read_dispatch_envelope(nmp_app_chirp_dispatch_action_bytes(
        app,
        namespace.as_ptr(),
        body.as_ptr(),
    ));
    let id = value
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {value}"));
    assert!(!id.is_empty());

    super::super::nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// D6: a null `app` must NOT crash — the byte doorway returns an `{"error"}`
/// envelope (`dispatch_action_bytes_for` rejects the null pointer).
#[test]
fn dispatch_action_bytes_null_app_returns_error_envelope() {
    let namespace = CString::new("nmp.follow").unwrap();
    let body = CString::new(r#"{"pubkey":"deadbeef"}"#).unwrap();
    let value = read_dispatch_envelope(nmp_app_chirp_dispatch_action_bytes(
        std::ptr::null_mut(),
        namespace.as_ptr(),
        body.as_ptr(),
    ));
    assert!(
        value.get("error").is_some(),
        "null app must return an error envelope, got {value}"
    );
    assert!(value.get("correlation_id").is_none());
}
