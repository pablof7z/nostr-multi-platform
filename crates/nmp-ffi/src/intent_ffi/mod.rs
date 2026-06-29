//! Input-intent resolver C-ABI (issue #1804).
//!
//! Two symbols bridge the one-box / paste / search field to the native runtime's
//! input-intent classification and dispatch API:
//!
//! * [`nmp_app_intent_dispatch`] — calls the runtime dispatch API and returns
//!   the chosen candidate (or the rejection) as JSON. See [`dispatch`].
//!
//! The remaining C ABI door returns a heap-owned C string the caller MUST release through
//! `nmp_free_string`. D6: never NULL; a malformed/missing argument yields a small
//! `{"ok":false,"error":"…"}` object rather than a panic.

mod dispatch;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use crate::NmpApp;
use serde::Serialize;
use std::ffi::{c_char, CStr, CString};

/// Classify `request_json`, then ask the native runtime to dispatch the top
/// candidate and return the chosen candidate (or the rejection) as JSON.
///
/// Routing of the top candidate (the first of `Candidates`):
/// Runtime routing of the top candidate:
/// * `DirectRef` → kernel `OpenUri`,
/// * `TextQuery` → native-runtime search session open,
/// * `Nip05` → NIP-05 reverse lookup command,
/// * `RelayUrl` / `Registered` → no generic side effect; the candidate is
///   returned for the host or owning crate to route.
///
/// `session_id` keys the search session when the top candidate is a `TextQuery`
/// (ignored otherwise). The returned JSON is
/// `{"ok":true,"dispatched":<candidate>}` or `{"ok":true,"rejection":<rejection>}`
/// or, when the input classified into zero candidates,
/// `{"ok":true,"rejection":"Unparseable"}`.
///
/// The returned C string is heap-owned by Rust and MUST be released through
/// `nmp_free_string`. D6: never NULL.
///
/// # Safety
/// `app` must be a valid pointer from [`crate::nmp_app_new`] (or null);
/// `request_json` / `session_id` must be valid NUL-terminated C strings (or null).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_intent_dispatch(
    app: *mut NmpApp,
    request_json: *const c_char,
    session_id: *const c_char,
) -> *mut c_char {
    let output = match dispatch::dispatch_request(app, request_json, session_id) {
        Ok(value) => value,
        Err(error) => error_json(error),
    };
    into_c_string(output)
}

#[derive(Serialize)]
struct FfiError {
    ok: bool,
    error: &'static str,
}

pub(crate) fn error_json(error: &'static str) -> String {
    serde_json::to_string(&FfiError { ok: false, error })
        .unwrap_or_else(|_| r#"{"ok":false,"error":"serialization-failed"}"#.to_string())
}

pub(crate) fn into_c_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        // The only failure is an interior NUL in `value` (never in our JSON);
        // fall back to a static C-string literal (infallible, no `.expect`).
        Err(_) => {
            const FALLBACK: &CStr = c"{\"ok\":false,\"error\":\"serialization-failed\"}";
            FALLBACK.to_owned().into_raw()
        }
    }
}
