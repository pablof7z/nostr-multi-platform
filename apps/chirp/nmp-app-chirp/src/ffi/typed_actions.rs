//! C-ABI bridge for Rust-authored Chirp action specs.
//!
//! Native shells pass typed user intent JSON and receive a serialized
//! [`crate::action_specs::ActionDispatchSpec`] with the exact action namespace
//! and body JSON Rust wants dispatched through `nmp_app_dispatch_action`.
//!
//! ## ADR-0064 / Cut-B host slice (#1782) — typed byte doorway
//!
//! [`nmp_app_chirp_dispatch_intent_bytes`] and
//! [`nmp_app_chirp_dispatch_action_bytes`] fold the intent→spec conversion AND
//! the spec→bytes dispatch into a SINGLE C call so no protocol logic (NIP-10
//! tags, kind:6 reposts, profile fields) leaks into the Swift host and no JSON
//! crosses to the kernel. The native host carries only raw user intent (a
//! `ChirpActionIntent`) or a pre-built `(namespace, body_json)` pair; Rust
//! authors the protocol body via [`crate::action_specs`] and dispatches the
//! typed [`ActionPayload`](nmp_core::substrate::ActionPayload) bytes through
//! [`crate::dispatch_bytes::dispatch_action_bytes_for`]. The retired path
//! (`nmp_app_chirp_action_spec` + a separate `nmp_app_dispatch_action`) is left
//! exported this slice for the not-yet-migrated host; it becomes dead once the
//! host stops calling it and is retired in a later cleanup.

use std::ffi::{c_char, CStr, CString};

use serde_json::json;

use nmp_ffi::NmpApp;

use crate::action_specs::{action_spec_for_intent_json, action_spec_json_for_intent};
use crate::dispatch_bytes::dispatch_action_bytes_for;

/// Build a Rust-owned Chirp action dispatch spec from typed intent JSON.
///
/// Returns `{"namespace":"...","body_json":"..."}` on success or
/// `{"error":"..."}` on malformed intent. The returned pointer must be freed
/// by the shell with `nmp_free_string`.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_action_spec(intent_json: *const c_char) -> *mut c_char {
    let result = read_c_string(intent_json)
        .map(|intent| action_spec_json_for_intent(&intent))
        .unwrap_or_else(|| r#"{"error":"missing Chirp action intent JSON"}"#.to_string());
    CString::new(result)
        .unwrap_or_else(|_| CString::new(r#"{"error":"invalid action spec string"}"#).unwrap_or_default())
        .into_raw()
}

/// Convert raw Chirp user intent into a typed action and dispatch it through
/// the byte doorway in one call.
///
/// Reads `intent_json` (a serialized `ChirpActionIntent`), builds the canonical
/// `(namespace, body_json)` spec via [`action_spec_for_intent_json`] (Rust owns
/// the protocol body — NIP-10 reply tags, kind:6 reposts, profile fields), then
/// hands the spec to [`dispatch_action_bytes_for`], which encodes the typed
/// [`ActionPayload`](nmp_core::substrate::ActionPayload) bytes, wraps them in an
/// open `DispatchEnvelope`, and calls the typed
/// [`nmp_ffi::nmp_app_dispatch_action_bytes`] doorway. No JSON crosses the FFI.
///
/// Returns `{"correlation_id":"<id>"}` on accept (the host-minted id echoed
/// verbatim) or `{"error":"<message>"}` on a malformed intent, an unknown /
/// mis-shaped namespace, or a kernel rejection — the SAME envelope shape the
/// host's `DispatchResult` parser already expects from the retired JSON lane.
/// The returned pointer must be freed by the shell with `nmp_free_string`.
///
/// # Safety
/// `app` must be a valid `*mut NmpApp` from `nmp_app_new`, or null. D6: a null
/// `app` returns an `{"error"}` envelope (never a crash) —
/// [`dispatch_action_bytes_for`] checks the pointer and returns
/// `"runtime app is not available"`. A null / empty `intent_json` returns an
/// `{"error"}` envelope.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_dispatch_intent_bytes(
    app: *mut NmpApp,
    intent_json: *const c_char,
) -> *mut c_char {
    let result = match read_c_string(intent_json) {
        Some(intent) => match action_spec_for_intent_json(&intent) {
            Ok(spec) => dispatch_action_bytes_for(app, &spec.namespace, &spec.body_json),
            Err(error) => Err(error),
        },
        None => Err("missing Chirp action intent JSON".to_string()),
    };
    dispatch_result_cstring(result)
}

/// Dispatch a pre-built Chirp action through the byte doorway.
///
/// For the direct-JSON dispatch sites (wallet, relay-lists, NIP-29 group ops)
/// where the host already holds a `(namespace, body_json)` pair and does NOT go
/// through the intent spec. Hands the pair to [`dispatch_action_bytes_for`],
/// which encodes the typed payload and dispatches the typed bytes. No JSON
/// crosses the FFI.
///
/// Returns the same `{"correlation_id"}` / `{"error"}` envelope as
/// [`nmp_app_chirp_dispatch_intent_bytes`]. The returned pointer must be freed
/// by the shell with `nmp_free_string`.
///
/// # Safety
/// `app` must be a valid `*mut NmpApp` from `nmp_app_new`, or null. D6: a null
/// `app` returns an `{"error"}` envelope. A null / empty `namespace` becomes
/// `""`, which [`dispatch_action_bytes_for`] rejects fail-closed.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_dispatch_action_bytes(
    app: *mut NmpApp,
    namespace: *const c_char,
    body_json: *const c_char,
) -> *mut c_char {
    let namespace = read_c_string(namespace).unwrap_or_default();
    let body_json = read_c_string(body_json).unwrap_or_default();
    let result = dispatch_action_bytes_for(app, &namespace, &body_json);
    dispatch_result_cstring(result)
}

/// Render a dispatch result as the canonical `{"correlation_id"}` /
/// `{"error"}` JSON envelope, owned by an `into_raw` `CString` the caller frees
/// with `nmp_free_string`. `serde_json::json!` keeps the message escape-safe.
fn dispatch_result_cstring(result: Result<String, String>) -> *mut c_char {
    let value = match result {
        Ok(correlation_id) => json!({ "correlation_id": correlation_id }),
        Err(error) => json!({ "error": error }),
    };
    CString::new(value.to_string())
        .unwrap_or_else(|_| {
            CString::new(r#"{"error":"invalid dispatch result string"}"#).unwrap_or_default()
        })
        .into_raw()
}

fn read_c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}
