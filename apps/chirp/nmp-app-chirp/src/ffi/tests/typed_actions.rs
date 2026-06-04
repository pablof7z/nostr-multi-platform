//! Native-facing Chirp action-spec builder tests.

use std::ffi::{CStr, CString};

use nmp_ffi::nmp_app_free_string;

use super::super::nmp_app_chirp_action_spec;

fn build_spec(intent: &str) -> serde_json::Value {
    let intent = CString::new(intent).unwrap();
    let ptr = nmp_app_chirp_action_spec(intent.as_ptr());
    assert!(!ptr.is_null(), "action spec must return a JSON string");
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    nmp_app_free_string(ptr);
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
    nmp_app_free_string(ptr);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["error"], "missing Chirp action intent JSON");
}
