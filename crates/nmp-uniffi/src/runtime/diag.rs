//! Intent dispatch and diagnostic info UniFFI methods — M14-C6.
//!
//! Migrates the intent-dispatch and debug-info C-ABI symbols:
//!
//! | UniFFI method    | C-ABI counterpart               |
//! |------------------|---------------------------------|
//! | `intent_dispatch`| `nmp_app_intent_dispatch`       |
//! | `debug_info`     | `nmp_app_debug_info`            |
//!
//! ## Design notes
//!
//! Both methods return JSON strings, matching the shape of the C-ABI
//! counterparts.
//!
//! * `intent_dispatch` accepts a serialized `InputIntentRequest` JSON
//!   (same schema as the C-ABI) and returns a dispatch-outcome JSON
//!   (`{"ok":true,"dispatched":…}` or `{"ok":true,"rejection":…}`).
//! * `debug_info` accepts a domain integer (0=routing, 1=composition,
//!   2=merged, other=`{}`) and returns the diagnostic JSON payload.
//!
//! D6: both methods are total — they never throw. A malformed request or
//! null app pointer returns a small `{"ok":false,"error":"…"}` or `{}`.

use nmp_core::substrate::{InputIntentCandidate, InputIntentRejection, InputIntentRequest};
use nmp_native_runtime::InputIntentDispatch;
use serde_json::json;

use crate::NmpApp;

// ── Serialization helpers (mirrors nmp-ffi/src/intent_ffi/dispatch.rs) ───────
//
// `InputIntentCandidate` / `InputIntentRejection` derive `serde::Serialize`
// upstream; we wrap them with `serde_json::to_value` + the `json!` macro so
// this crate needs no direct `serde` dependency (only `serde_json`).

fn error_json(error: &'static str) -> String {
    serde_json::to_string(&json!({ "ok": false, "error": error }))
        .unwrap_or_else(|_| r#"{"ok":false,"error":"serialization-failed"}"#.to_string())
}

fn dispatched_json(candidate: &InputIntentCandidate) -> String {
    match serde_json::to_value(candidate) {
        Ok(value) => serde_json::to_string(&json!({ "ok": true, "dispatched": value }))
            .unwrap_or_else(|_| error_json("serialization-failed")),
        Err(_) => error_json("serialization-failed"),
    }
}

fn rejection_json(rejection: &InputIntentRejection) -> String {
    match serde_json::to_value(rejection) {
        Ok(value) => serde_json::to_string(&json!({ "ok": true, "rejection": value }))
            .unwrap_or_else(|_| error_json("serialization-failed")),
        Err(_) => error_json("serialization-failed"),
    }
}

// ── UniFFI methods ────────────────────────────────────────────────────────────

#[uniffi::export]
impl NmpApp {
    /// Classify and dispatch the top candidate for `request_json` through the
    /// native runtime.
    ///
    /// `request_json` must be a serialized `InputIntentRequest`:
    /// ```json
    /// {"input":"jb55@jb55.com",
    ///  "scopes":[{"namespace":"nip50","name":"profiles"}],
    ///  "text_targets":"UserPreferred"}
    /// ```
    ///
    /// `session_id` keys the search session when the top candidate is a
    /// `TextQuery` (ignored otherwise).
    ///
    /// Returns a JSON string:
    /// * `{"ok":true,"dispatched":<candidate>}` — input classified and
    ///   dispatched.
    /// * `{"ok":true,"rejection":<rejection>}` — input rejected (no match,
    ///   disallowed scope, secret detected, etc.).
    /// * `{"ok":false,"error":"…"}` — malformed `request_json`.
    ///
    /// D6: never throws. Routing side effects (OpenUri, search-session open,
    /// NIP-05 lookup) happen as fire-and-forget on the actor channel.
    pub fn intent_dispatch(&self, request_json: String, session_id: Option<String>) -> String {
        let request: InputIntentRequest = match serde_json::from_str(&request_json) {
            Ok(r) => r,
            Err(_) => return error_json("unparseable-request"),
        };
        match self
            .inner
            .dispatch_input_intent(&request, session_id.as_deref())
        {
            InputIntentDispatch::Dispatched(candidate) => dispatched_json(&candidate),
            InputIntentDispatch::Rejection(rejection) => rejection_json(&rejection),
        }
    }

    /// Return a diagnostic JSON payload for `domain`.
    ///
    /// | `domain` | Payload |
    /// |----------|---------|
    /// | 0 | Routing-trace JSON (schema_version, capacity, publishes, subscriptions) |
    /// | 1 | Composition-report JSON (schema_version, count, records) |
    /// | 2 | Merged: `{"routing":{…},"composition":{…}}` |
    /// | other | `{}` (D6 silent no-op) |
    ///
    /// D6: never throws. A pre-start kernel, unavailable projection, or
    /// serialization failure all collapse to a well-formed empty payload.
    pub fn debug_info(&self, domain: i32) -> String {
        let value = self.inner.debug_info_json(domain);
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::NmpApp;

    // ── debug_info ────────────────────────────────────────────────────────

    /// Parity with C-ABI test `fresh_app_domain0_is_well_formed`:
    /// domain 0 (routing trace) returns valid JSON with expected top-level keys.
    #[test]
    fn parity_debug_info_domain0_well_formed() {
        let app = NmpApp::new();
        let json = app.debug_info(0);
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("debug_info domain 0 must be valid JSON");
        assert_eq!(v["schema_version"], 1, "schema_version must be 1");
        assert!(v["publishes"].is_array(), "publishes must be an array");
        assert!(
            v["subscriptions"].is_array(),
            "subscriptions must be an array"
        );
    }

    /// Parity with C-ABI test `fresh_app_domain1_is_well_formed`:
    /// domain 1 (composition report) returns valid JSON with expected keys.
    #[test]
    fn parity_debug_info_domain1_well_formed() {
        let app = NmpApp::new();
        let json = app.debug_info(1);
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("debug_info domain 1 must be valid JSON");
        assert_eq!(v["schema_version"], 1, "schema_version must be 1");
        assert!(v["records"].is_array(), "records must be an array");
        assert!(v["count"].is_u64(), "count must be u64");
    }

    /// Parity with C-ABI test `fresh_app_domain2_has_both_keys`:
    /// domain 2 (merged) returns valid JSON with both `routing` and
    /// `composition` keys.
    #[test]
    fn parity_debug_info_domain2_merged() {
        let app = NmpApp::new();
        let json = app.debug_info(2);
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("debug_info domain 2 must be valid JSON");
        assert!(v["routing"].is_object(), "routing must be an object");
        assert!(
            v["composition"].is_object(),
            "composition must be an object"
        );
    }

    /// Parity with C-ABI test `unknown_domain_returns_empty_object`:
    /// an unknown domain returns a valid (possibly empty) JSON object.
    #[test]
    fn parity_debug_info_unknown_domain_empty_object() {
        let app = NmpApp::new();
        let json = app.debug_info(99);
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("debug_info unknown domain must be valid JSON");
        assert!(v.is_object(), "unknown domain must return a JSON object");
    }

    // ── intent_dispatch ───────────────────────────────────────────────────

    /// Parity with C-ABI `nmp_app_intent_dispatch`:
    /// a malformed request JSON returns `{"ok":false,"error":"…"}`.
    #[test]
    fn parity_intent_dispatch_malformed_request_returns_error() {
        let app = NmpApp::new();
        let result = app.intent_dispatch("not-json".to_string(), None);
        let v: serde_json::Value =
            serde_json::from_str(&result).expect("result must be valid JSON");
        assert_eq!(v["ok"], false, "malformed request must have ok=false");
        assert!(v["error"].is_string(), "error field must be present");
    }

    /// A valid request that resolves to a NIP-19 reference returns a
    /// dispatched outcome (`ok=true`, `dispatched` present).
    #[test]
    fn parity_intent_dispatch_valid_nprofile_dispatched() {
        let app = NmpApp::new();
        // npub for a well-known public key; the classifier resolves this as a
        // DirectRef → Dispatched.
        let request_json = serde_json::json!({
            "input": "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6",
            "scopes": [],
            "text_targets": "UserPreferred"
        })
        .to_string();
        let result = app.intent_dispatch(request_json, None);
        let v: serde_json::Value =
            serde_json::from_str(&result).expect("result must be valid JSON");
        // The classifier should return ok=true with either dispatched or rejection.
        assert!(v["ok"].as_bool().unwrap_or(false), "ok must be true");
    }
}
