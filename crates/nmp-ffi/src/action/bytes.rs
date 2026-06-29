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
//! The dispatch core lives in
//! [`nmp_native_runtime::action_dispatch`] so the C-ABI and the UniFFI surface
//! share a single typed implementation. This file contains only the JSON
//! serialisation layer retained for crate-local tests.

use super::super::NmpApp;
use super::error_json;
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

/// Delegate to the typed core in `nmp-native-runtime` and serialise the
/// [`DispatchOutcome`] to the legacy JSON result shape for focused tests.
pub(in crate::action) fn dispatch_action_bytes(app: Option<&NmpApp>, bytes: &[u8]) -> String {
    let Some(app) = app else {
        return error_json("null app");
    };
    outcome_to_json(dispatch_action_bytes_typed(app, bytes))
}

#[cfg(test)]
#[path = "tests_bytes.rs"]
mod tests_bytes;
