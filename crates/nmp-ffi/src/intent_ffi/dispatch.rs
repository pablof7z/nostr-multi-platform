//! Dispatch lane for [`super::nmp_app_intent_dispatch`] (issue #1804).
//!
//! The C ABI layer parses pointer/string inputs, calls
//! [`NmpApp::dispatch_input_intent`], then serializes the runtime-owned outcome.

use crate::{app_ref, c_string_argument, NmpApp};
use nmp_core::substrate::{InputIntentCandidate, InputIntentRejection, InputIntentRequest};
use nmp_native_runtime::InputIntentDispatch;
use serde::Serialize;

/// `nmp_app_intent_dispatch` body. Returns the result JSON string (the C-string
/// wrapping happens in the caller). Argument errors propagate as `Err(&str)` →
/// the caller renders a `{"ok":false,"error":"…"}` object.
pub(crate) fn dispatch_request(
    app: *mut NmpApp,
    request_json: *const std::ffi::c_char,
    session_id: *const std::ffi::c_char,
) -> Result<String, &'static str> {
    let app = app_ref(app).ok_or("invalid-app")?;
    let request_json = c_string_argument(request_json).ok_or("invalid-input")?;
    let request: InputIntentRequest =
        serde_json::from_str(&request_json).map_err(|_| "unparseable-request")?;
    let session_id = c_string_argument(session_id);
    match app.dispatch_input_intent(&request, session_id.as_deref()) {
        InputIntentDispatch::Dispatched(candidate) => Ok(dispatched_json(&candidate)),
        InputIntentDispatch::Rejection(rejection) => Ok(rejection_json(&rejection)),
    }
}

#[derive(Serialize)]
struct Dispatched<'a> {
    ok: bool,
    dispatched: &'a InputIntentCandidate,
}

#[derive(Serialize)]
struct Rejected<'a> {
    ok: bool,
    rejection: &'a InputIntentRejection,
}

fn dispatched_json(candidate: &InputIntentCandidate) -> String {
    serde_json::to_string(&Dispatched {
        ok: true,
        dispatched: candidate,
    })
    .unwrap_or_else(|_| super::error_json("serialization-failed"))
}

fn rejection_json(rejection: &InputIntentRejection) -> String {
    serde_json::to_string(&Rejected {
        ok: true,
        rejection,
    })
    .unwrap_or_else(|_| super::error_json("serialization-failed"))
}
