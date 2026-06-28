//! C-ABI byte doorway for pre-built Chirp actions.
//!
//! ## ADR-0064 / Cut-B host slice (#1782), M14-1 / #2145
//!
//! [`nmp_app_chirp_dispatch_action_bytes`] dispatches a pre-built
//! `(namespace, body_json)` pair through the typed byte doorway: it hands the
//! pair to [`dispatch_action_bytes_for`], which encodes the namespace's typed
//! [`ActionPayload`](nmp_core::substrate::ActionPayload) bytes, wraps them in an
//! open `DispatchEnvelope`, and calls the typed
//! [`nmp_ffi::nmp_app_dispatch_action_bytes`] doorway. No JSON crosses to the
//! kernel.
//!
//! This is the raw doorway for the direct-dispatch sites (NIP-29 group ops,
//! #2170) where the host already holds a `(namespace, body_json)` pair. Social
//! writes (notes, reactions, reposts, follows, zaps, DMs) no longer route here:
//! they ride the generated `GeneratedActionBuilders` FlatBuffers byte builders
//! straight to the byte doorway. The former `ChirpActionIntent` JSON intent lane
//! (`nmp_app_chirp_action_spec` / `nmp_app_chirp_dispatch_intent_bytes`) has been
//! retired (M14-1 PR2 / #2145).

use std::ffi::{c_char, CStr, CString};

use serde_json::json;

use nmp_ffi::NmpApp;

use crate::dispatch_bytes::dispatch_action_bytes_for;

/// Dispatch a pre-built Chirp action through the byte doorway.
///
/// For the direct-JSON dispatch sites (NIP-29 group ops) where the host already
/// holds a `(namespace, body_json)` pair. Hands the pair to
/// [`dispatch_action_bytes_for`], which encodes the typed payload and dispatches
/// the typed bytes. No JSON crosses the FFI.
///
/// Returns `{"correlation_id":"<id>"}` on accept (the host-minted id echoed
/// verbatim) or `{"error":"<message>"}` on an unknown / mis-shaped namespace or
/// a kernel rejection. The returned pointer must be freed by the shell with
/// `nmp_free_string`.
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
