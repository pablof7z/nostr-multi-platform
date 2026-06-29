//! ADR-0064 / S4 (#1752) — the native **byte** doorway.
//!
//! The typed-FlatBuffers twin of the JSON `nmp_app_dispatch_action` doorway
//! (`action.rs`). It carries an open
//! [`nmp_core::dispatch_envelope::DispatchEnvelope`] (correlation_id + generated
//! `action_namespace` + schema_version + opaque per-crate payload) instead of
//! `(namespace, action_json)`, decodes it (S2), routes the opaque payload by
//! namespace into the registry's typed `start_bytes` / `execute_bytes` (S3),
//! and returns the SAME `{"correlation_id":…}` / `{"error":…}` shape.
//!
//! The FFI-free dispatch core now lives in
//! [`nmp_native_runtime::action_dispatch`] so the C-ABI and the UniFFI surface
//! share a single typed implementation. This file contains only the C-ABI
//! entry point and the JSON serialisation layer.

use std::ffi::{c_char, CString};

use super::super::{app_ref, NmpApp};
use super::error_json;
use nmp_core::dispatch_envelope::MAX_DISPATCH_ENVELOPE_BYTES;
use nmp_native_runtime::{dispatch_action_bytes_typed, DispatchOutcome};

/// Serialise a [`DispatchOutcome`] to the `{"correlation_id":…}` /
/// `{"error":…}` / `{"error":…,"code":…}` JSON shape expected by the C-ABI
/// callers. This is the sole JSON serialisation point for the byte doorway.
pub(in crate::action) fn outcome_to_json(outcome: DispatchOutcome) -> String {
    use super::json_string;
    match (outcome.correlation_id, outcome.error, outcome.code) {
        (Some(cid), None, None) => {
            format!(r#"{{"correlation_id":{}}}"#, json_string(&cid))
        }
        (Some(cid), Some(err), None) => {
            // Post-mint failure: both correlation_id and error are present so
            // the host can ACK the stage and show a toast.
            format!(
                r#"{{"correlation_id":{},"error":{}}}"#,
                json_string(&cid),
                json_string(&err),
            )
        }
        (None, Some(err), Some(code)) => {
            format!(
                r#"{{"error":{},"code":{}}}"#,
                json_string(&err),
                json_string(&code),
            )
        }
        (None, Some(err), None) => {
            format!(r#"{{"error":{}}}"#, json_string(&err))
        }
        // Uninhabited states — return a fallback error (D6).
        _ => r#"{"error":"internal: malformed dispatch outcome"}"#.to_string(),
    }
}

/// ADR-0064 / S4 (#1752) — dispatch a typed action through the **byte**
/// doorway.
///
/// Returns a freshly heap-allocated, NUL-terminated JSON C string the caller
/// MUST release via [`crate::free::nmp_free_string`]:
///
/// * `{"correlation_id":"<id>"}` — accepted and enqueued.
/// * `{"error":"<message>"}` — rejected. Fail-closed (D6): a null `app`, a
///   null `ptr`, an oversize / malformed / wrong-identifier / wrong-schema-
///   version / namespace-less / correlation-id-less envelope, an unknown
///   namespace, or a not-typed-capable module all come back here. An oversize
///   `len` is rejected BEFORE a slice is even formed.
/// * `{"error":"<msg>","code":"<token>"}` — coded rejection (issue #1734).
///
/// # Safety
/// `app` must be a valid non-null pointer from [`crate::nmp_app_new`], or null.
/// `ptr`/`len` must describe a valid readable byte range, or `ptr` may be null
/// with `len` `0` (treated as an empty buffer and rejected).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_dispatch_action_bytes(
    app: *mut NmpApp,
    ptr: *const u8,
    len: usize,
) -> *mut c_char {
    // Fail-closed BEFORE forming a slice: a hostile `len` must never construct
    // a `&[u8]` of that span. The S2 decoder bounds the same MAX ceiling, but
    // gating it here means an oversize length can never drive slice creation.
    if len > MAX_DISPATCH_ENVELOPE_BYTES {
        use nmp_core::dispatch_envelope::DispatchDecodeError;
        let result = error_json(
            &DispatchDecodeError::Oversize {
                len,
                max: MAX_DISPATCH_ENVELOPE_BYTES,
            }
            .to_string(),
        );
        return CString::new(result)
            .unwrap_or_else(|_| c"{}".to_owned())
            .into_raw();
    }
    // A null `ptr` (or zero `len`) is an empty buffer — the S2 decoder rejects
    // it fail-closed (BadFileIdentifier), so we never dereference a null.
    let bytes: &[u8] = if ptr.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller's safety contract guarantees `ptr`/`len` describe
        // a valid readable byte range for the duration of this call.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    };
    let result = dispatch_action_bytes(app_ref(app), bytes);
    CString::new(result)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

/// Pure (FFI-free) shim — delegate to the typed core in `nmp-native-runtime`
/// and serialise the [`DispatchOutcome`] to JSON for the C-ABI callers.
///
/// Split out so the unit tests can exercise the JSON-serialisation path without
/// raw pointers (same as the previous `dispatch_action_bytes` test seam).
pub(in crate::action) fn dispatch_action_bytes(app: Option<&NmpApp>, bytes: &[u8]) -> String {
    let Some(app) = app else {
        return error_json("null app");
    };
    outcome_to_json(dispatch_action_bytes_typed(app, bytes))
}

#[cfg(test)]
#[path = "tests_bytes.rs"]
mod tests_bytes;
