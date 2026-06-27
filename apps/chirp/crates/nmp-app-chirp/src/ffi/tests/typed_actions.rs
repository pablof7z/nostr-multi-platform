//! Chirp `(namespace, body_json)` byte-doorway dispatch tests.
//!
//! M14-1 / PR2 (#2145): the JSON-intent C symbols (`nmp_app_chirp_action_spec`,
//! `nmp_app_chirp_dispatch_intent_bytes`) were retired; the only residual C
//! doorway is `nmp_app_chirp_dispatch_action_bytes`, which the in-repo Rust
//! shells use for pre-built `(namespace, body_json)` pairs.

use std::ffi::CStr;
use std::ffi::CString;

use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_free_string};

use super::super::nmp_app_chirp_dispatch_action_bytes;
use super::helpers::register_app;

const HEX64: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Read + free a `*mut c_char` dispatch envelope into a parsed JSON value.
fn read_dispatch_envelope(ptr: *mut std::ffi::c_char) -> serde_json::Value {
    assert!(!ptr.is_null(), "dispatch envelope must return a JSON string");
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
    nmp_free_string(ptr);
    serde_json::from_str(&out).unwrap()
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

/// The new `nmp.nip01.publish_note` namespace dispatches a kind:1 note through
/// the byte doorway (the Rust-shell JSON→bytes encoder added in M14-1 / PR2) and
/// echoes a non-empty correlation id — proving the host-registered module is
/// wired.
#[test]
fn dispatch_action_bytes_publish_note_returns_correlation_id() {
    let app = nmp_app_new();
    let handle = register_app(app);

    let namespace = CString::new("nmp.nip01.publish_note").unwrap();
    let body = CString::new(r#"{"content":"hello from the byte doorway"}"#).unwrap();
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

/// The new `nmp.nip18.repost` namespace dispatches a kind:6 repost through the
/// byte doorway and echoes a non-empty correlation id.
#[test]
fn dispatch_action_bytes_repost_returns_correlation_id() {
    let app = nmp_app_new();
    let handle = register_app(app);

    let namespace = CString::new("nmp.nip18.repost").unwrap();
    let body = CString::new(format!(
        r#"{{"event_id":"{HEX64}","author_pubkey":"{HEX64}"}}"#
    ))
    .unwrap();
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
