//! Dispatch lane for [`super::nmp_app_intent_dispatch`] (issue #1804).
//!
//! Classifies the request, then routes the TOP candidate (the first of
//! `Candidates`) to the matching seam and returns the chosen candidate (or the
//! rejection) as JSON. Every routed target reuses an existing single seam — no
//! new dispatch primitive is introduced here:
//!
//! * `DirectRef`  → `KernelAction::OpenUri` (same as `nmp_app_open_uri`).
//! * `TextQuery`  → `NmpApp::open_search` (same as `nmp_app_search_open`).
//! * `Nip05`      → `ActorCommand::Protocol(ResolveNip05Command)` (HTTP reverse
//!   lookup → follow-up `ResolveRef` profile claim).
//! * `RelayUrl` / `Registered` → no in-FFI seam exists; the candidate is
//!   returned verbatim for the host to route (a relay-metadata view, or the
//!   owning recognizer crate's handler / relay-pin lane).

use crate::{app_ref, c_string_argument, NmpApp};
use nmp_core::substrate::{
    InputIntentCandidate, InputIntentClassification, InputIntentRejection, InputIntentTarget,
};
use nmp_core::actor::ActorCommand;
use serde::Serialize;

use super::classify_request;

/// `nmp_app_intent_dispatch` body. Returns the result JSON string (the C-string
/// wrapping happens in the caller). Argument errors propagate as `Err(&str)` →
/// the caller renders a `{"ok":false,"error":"…"}` object.
pub(crate) fn dispatch_request(
    app: *mut NmpApp,
    request_json: *const std::ffi::c_char,
    session_id: *const std::ffi::c_char,
) -> Result<String, &'static str> {
    let classification = classify_request(app, request_json)?;
    // SAFETY/validity already established by `classify_request`.
    let app = app_ref(app).ok_or("invalid-app")?;
    let session_id = c_string_argument(session_id);

    match classification {
        InputIntentClassification::Rejection(rejection) => Ok(rejection_json(&rejection)),
        InputIntentClassification::Candidates(candidates) => match candidates.into_iter().next() {
            // Zero candidates is not a value the classifier emits (it returns a
            // `Rejection` instead), but stay total: treat an empty list as
            // unparseable rather than panicking.
            None => Ok(rejection_json(&InputIntentRejection::Unparseable)),
            Some(candidate) => {
                act_on(app, &candidate, session_id.as_deref());
                Ok(dispatched_json(&candidate))
            }
        },
    }
}

/// Route the chosen candidate's target to its seam. Side-effecting; `RelayUrl`
/// and `Registered` have no in-FFI seam and are returned to the host unrouted.
fn act_on(app: &NmpApp, candidate: &InputIntentCandidate, session_id: Option<&str>) {
    match &candidate.target {
        InputIntentTarget::DirectRef { uri } => {
            app.send_cmd(ActorCommand::Kernel(nmp_core::KernelAction::OpenUri {
                uri: uri.clone(),
            }));
        }
        InputIntentTarget::TextQuery { request_json } => {
            // Re-validate the opaque SearchRequest JSON through the same NIP-50
            // bounded-query constructor `nmp_app_search_open` uses, then open a
            // session under `session_id`. A missing session id or a request that
            // fails validation is a no-op (D6) — the candidate is still reported
            // as dispatched so the host can read its own session key.
            if let Some(session_id) = session_id.filter(|s| !s.is_empty()) {
                if let Some(request) = crate::search::parse_search_request(request_json) {
                    let _ = app.open_search(request, session_id);
                }
            }
        }
        InputIntentTarget::Nip05 { identifier } => {
            // Re-split the shape-validated identifier (pure, no IO) and enqueue
            // the reverse-lookup ProtocolCommand. The worker performs the HTTP
            // GET off the actor thread and posts a follow-up profile ResolveRef.
            if let Some((name, domain)) = nmp_nip05::parse_nip05(identifier) {
                app.send_cmd(ActorCommand::Protocol(Box::new(
                    nmp_nip05::ResolveNip05Command {
                        name,
                        domain,
                        correlation_id: None,
                    },
                )));
            }
        }
        // No in-FFI seam: the host routes these from the returned candidate JSON
        // (RelayUrl → a relay-metadata view; Registered → the owning crate's
        // handler / relay-pin lane).
        InputIntentTarget::RelayUrl { .. } | InputIntentTarget::Registered { .. } => {}
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
