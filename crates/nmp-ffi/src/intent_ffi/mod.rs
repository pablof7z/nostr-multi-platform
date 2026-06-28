//! Input-intent resolver C-ABI (issue #1804).
//!
//! Two symbols bridge the one-box / paste / search field to the native runtime's
//! input-intent classification and dispatch API:
//!
//! * [`nmp_app_intent_classify`] — STATELESS / sync / side-effect-free. Parses a
//!   request JSON into an [`InputIntentRequest`], snapshots the app's registered
//!   recognizers, runs `classify`, and returns the bounded
//!   [`InputIntentClassification`] as JSON. Mirrors the decode-only posture of
//!   [`crate::nip21_ffi::nmp_nip21_decode_uri`]: it never mutates kernel/view
//!   state and never touches the network. A `SecretLike` rejection carries **no**
//!   copy of the input — the returned JSON never echoes the secret.
//! * [`nmp_app_intent_dispatch`] — calls the runtime dispatch API and returns
//!   the chosen candidate (or the rejection) as JSON. See [`dispatch`].
//!
//! Both return a heap-owned C string the caller MUST release through
//! `nmp_free_string`. D6: never NULL; a malformed/missing argument yields a small
//! `{"ok":false,"error":"…"}` object rather than a panic.

mod dispatch;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use crate::{app_ref, c_string_argument, NmpApp};
use nmp_core::substrate::{InputIntentClassification, InputIntentRequest};
use serde::Serialize;
use std::ffi::{c_char, CStr, CString};

/// Classify one untyped input string against the app's registered recognizers.
///
/// `request_json` is the serde JSON of an [`InputIntentRequest`]:
///
/// ```json
/// {"input":"jb55@jb55.com",
///  "scopes":[{"namespace":"nip50","name":"profiles"}],
///  "text_targets":"UserPreferred"}
/// ```
///
/// (`text_targets` accepts `"UserPreferred"`, `"AppDefault"`,
/// `{"Explicit":["wss://…"]}`.)
///
/// Returns the [`InputIntentClassification`] as JSON (a `Candidates` list or a
/// single `Rejection`). This symbol is STATELESS: it reads the registered
/// recognizer snapshot and runs the PURE classifier — no kernel mutation, no IO.
///
/// A `SecretLike` rejection (`nsec` / `nostr:nsec` / `ncryptsec`) returns
/// `{"ok":true,"classification":{"Rejection":"SecretLike"}}` — the input string
/// is NEVER copied into the result, logged, or echoed.
///
/// The returned C string is heap-owned by Rust and MUST be released through
/// `nmp_free_string`. D6: never NULL; a null/invalid/malformed argument returns a
/// small `{"ok":false,"error":"…"}` object.
///
/// # Safety
/// `app` must be a valid pointer from [`crate::nmp_app_new`] (or null);
/// `request_json` must be a valid NUL-terminated C string (or null).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_intent_classify(
    app: *mut NmpApp,
    request_json: *const c_char,
) -> *mut c_char {
    let output = match classify_request(app, request_json) {
        Ok(classification) => ok_classification_json(&classification),
        Err(error) => error_json(error),
    };
    into_c_string(output)
}

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

/// Shared classify path: resolve the app, parse the request, and call the
/// runtime's pure classification API.
pub(crate) fn classify_request(
    app: *mut NmpApp,
    request_json: *const c_char,
) -> Result<InputIntentClassification, &'static str> {
    let app = app_ref(app).ok_or("invalid-app")?;
    let request_json = c_string_argument(request_json).ok_or("invalid-input")?;
    let request: InputIntentRequest =
        serde_json::from_str(&request_json).map_err(|_| "unparseable-request")?;
    Ok(app.classify_input_intent(&request))
}

#[derive(Serialize)]
struct ClassifyOk<'a> {
    ok: bool,
    classification: &'a InputIntentClassification,
}

#[derive(Serialize)]
struct FfiError {
    ok: bool,
    error: &'static str,
}

fn ok_classification_json(classification: &InputIntentClassification) -> String {
    serde_json::to_string(&ClassifyOk {
        ok: true,
        classification,
    })
    .unwrap_or_else(|_| error_json("serialization-failed"))
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
