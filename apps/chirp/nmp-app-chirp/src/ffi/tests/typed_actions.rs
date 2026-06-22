//! Native-facing Chirp action-spec builder tests.

use std::ffi::{CStr, CString};

use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_free_string};

use super::super::{
    nmp_app_chirp_action_spec, nmp_app_chirp_dispatch_action_bytes,
    nmp_app_chirp_dispatch_intent_bytes,
};
use super::helpers::register_app;

fn build_spec(intent: &str) -> serde_json::Value {
    let intent = CString::new(intent).unwrap();
    let ptr = nmp_app_chirp_action_spec(intent.as_ptr());
    assert!(!ptr.is_null(), "action spec must return a JSON string");
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    nmp_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}

/// Read + free a `*mut c_char` dispatch envelope into a parsed JSON value.
fn read_dispatch_envelope(ptr: *mut std::ffi::c_char) -> serde_json::Value {
    assert!(!ptr.is_null(), "dispatch envelope must return a JSON string");
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    nmp_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}

#[test]
fn action_spec_ffi_returns_dispatch_spec_for_native_intent() {
    let spec = build_spec(r#"{"type":"react","event_id":"event","reaction":"+"}"#);
    assert_eq!(spec["namespace"], "nmp.nip25.react");
    let body: serde_json::Value =
        serde_json::from_str(spec["body_json"].as_str().unwrap()).unwrap();
    assert_eq!(body["target_event_id"], "event");
    assert_eq!(body["reaction"], "+");
}

#[test]
fn action_spec_ffi_returns_error_for_null_intent() {
    let ptr = nmp_app_chirp_action_spec(std::ptr::null());
    assert!(!ptr.is_null());
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    nmp_free_string(ptr);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["error"], "missing Chirp action intent JSON");
}

const HEX64: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// ADR-0064 / Cut-B host slice (#1782): the intent doorway folds intent→spec
/// →typed-bytes into ONE C call. A real `react` intent dispatches through the
/// byte doorway and echoes a non-empty host-minted correlation id (not an
/// error) — proving the host-registered module + executor are wired and no JSON
/// crosses to the kernel.
#[test]
fn dispatch_intent_bytes_react_returns_correlation_id() {
    let app = nmp_app_new();
    let handle = register_app(app);

    let intent = CString::new(format!(
        r#"{{"type":"react","event_id":"{HEX64}","reaction":"+"}}"#
    ))
    .unwrap();
    let value = read_dispatch_envelope(nmp_app_chirp_dispatch_intent_bytes(app, intent.as_ptr()));
    let id = value
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {value}"));
    assert!(!id.is_empty(), "byte doorway must echo a non-empty correlation id");

    super::super::nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// D6: a null `app` must NOT crash — the intent doorway returns an `{"error"}`
/// envelope (`dispatch_action_bytes_for` rejects the null pointer).
#[test]
fn dispatch_intent_bytes_null_app_returns_error_envelope() {
    let intent = CString::new(format!(
        r#"{{"type":"react","event_id":"{HEX64}","reaction":"+"}}"#
    ))
    .unwrap();
    let value = read_dispatch_envelope(nmp_app_chirp_dispatch_intent_bytes(
        std::ptr::null_mut(),
        intent.as_ptr(),
    ));
    assert!(
        value.get("error").is_some(),
        "null app must return an error envelope, got {value}"
    );
    assert!(value.get("correlation_id").is_none());
}

/// The direct namespace+body doorway dispatches a known direct action
/// (`nmp.follow`) and echoes a non-empty correlation id.
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
