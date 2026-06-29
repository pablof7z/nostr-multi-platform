//! C-ABI tests for the #1804 input-intent dispatch surface: the SECRET-NO-ECHO
//! invariant at the FFI boundary and dispatch routing per top-candidate target
//! class.

use super::nmp_app_intent_dispatch;
use crate::free::nmp_free_string;
use crate::{test_app_free, test_app_new, NmpApp};
use nmp_core::nip19::{encode_naddr, encode_npub, encode_nsec, NaddrData};
use nmp_core::substrate::{
    InputIntentTarget, InputScopeId, InputScopeRecognizer, ResolvedInput, TextSearchTargets,
};
use serde_json::Value;
use std::ffi::{CStr, CString};
use std::sync::Arc;

const PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const SECRET_HEX: &str = "67dea2ed018072d675f5415ecfaed7d2597555e202d85b3d65ea4e58d2d92ffa";

fn cstr(value: &str) -> CString {
    CString::new(value).expect("fixture has no NUL")
}

fn read_and_free(ptr: *mut std::ffi::c_char) -> (String, Value) {
    assert!(!ptr.is_null(), "FFI must never return NULL (D6)");
    // SAFETY: `ptr` is a valid heap C string returned by the FFI under test.
    let raw = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("output is UTF-8")
        .to_owned();
    nmp_free_string(ptr);
    let value = serde_json::from_str(&raw).expect("output is valid JSON");
    (raw, value)
}

fn dispatch(app: *mut NmpApp, request_json: &str, session_id: &str) -> (String, Value) {
    let req = cstr(request_json);
    let sid = cstr(session_id);
    read_and_free(nmp_app_intent_dispatch(app, req.as_ptr(), sid.as_ptr()))
}

/// Build an `InputIntentRequest` JSON with the given input + scope labels.
fn request_json(input: &str, scopes: &[(&str, &str)]) -> String {
    let scopes: Vec<Value> = scopes
        .iter()
        .map(|(ns, name)| serde_json::json!({"namespace": ns, "name": name}))
        .collect();
    serde_json::json!({
        "input": input,
        "scopes": scopes,
        "text_targets": "UserPreferred",
    })
    .to_string()
}

// ── SECRET-NO-ECHO invariant at the FFI boundary ─────────────────────────────

#[test]
fn dispatch_secret_is_rejected_and_never_echoed() {
    let app = test_app_new();
    let nsec = encode_nsec(SECRET_HEX).unwrap();
    let (raw, value) = dispatch(
        app,
        &request_json(&nsec, &[("nip50", "profiles")]),
        "sess-secret",
    );
    assert_eq!(value["rejection"], "SecretLike");
    assert!(!raw.contains(&nsec));
    assert!(!raw.contains(SECRET_HEX));
    test_app_free(app);
}

// ── dispatch: routing per target class ───────────────────────────────────────

#[test]
fn dispatch_direct_ref_returns_dispatched_candidate() {
    let app = test_app_new();
    let npub = encode_npub(PUBKEY).unwrap();
    let (_, value) = dispatch(
        app,
        &request_json(&npub, &[("nip50", "profiles")]),
        "sess-ref",
    );
    assert_eq!(value["ok"], true);
    assert!(value["dispatched"]["target"]["DirectRef"]["uri"]
        .as_str()
        .unwrap()
        .contains("npub1"));
    test_app_free(app);
}

#[test]
fn dispatch_text_query_opens_search_session() {
    let app = test_app_new();
    let (_, value) = dispatch(
        app,
        &request_json("nostr", &[("nip50", "profiles")]),
        "sess-text",
    );
    // The chosen candidate is a TextQuery and is reported dispatched; opening the
    // session must not panic (no relay source installed → cache-only search).
    assert!(value["dispatched"]["target"]["TextQuery"]["request_json"].is_string());
    test_app_free(app);
}

#[test]
fn dispatch_nip05_returns_dispatched_candidate() {
    let app = test_app_new();
    let (_, value) = dispatch(
        app,
        &request_json("jb55@jb55.com", &[("nip50", "profiles")]),
        "sess-nip05",
    );
    // The Nip05 candidate is reported dispatched; enqueuing the reverse-lookup
    // ProtocolCommand must not panic.
    assert_eq!(
        value["dispatched"]["target"]["Nip05"]["identifier"],
        "jb55@jb55.com"
    );
    test_app_free(app);
}

#[test]
fn dispatch_relay_url_returns_unrouted_candidate() {
    let app = test_app_new();
    let (_, value) = dispatch(
        app,
        &request_json("wss://relay.example/", &[("nip50", "notes")]),
        "sess-relay",
    );
    // RelayUrl has no in-FFI seam; the candidate is returned for the host to
    // route (relay-metadata view). Normalized (trailing slash stripped).
    assert_eq!(
        value["dispatched"]["target"]["RelayUrl"]["url"],
        "wss://relay.example"
    );
    test_app_free(app);
}

// ── dispatch: refusal passthrough ────────────────────────────────────────────

#[test]
fn dispatch_disallowed_scope_returns_rejection() {
    let app = test_app_new();
    // A valid naddr (address-class ref) requested under a users-only scope set
    // → DisallowedScope.
    let naddr = encode_naddr(&NaddrData {
        identifier: "my-article".to_string(),
        pubkey: PUBKEY.to_string(),
        kind: 30023,
        relays: Vec::new(),
    })
    .unwrap();
    let (_, value) = dispatch(
        app,
        &request_json(&naddr, &[("nip50", "profiles")]),
        "sess-disallowed",
    );
    assert!(value["rejection"]["DisallowedScope"].is_object());
    test_app_free(app);
}

// ── dispatch: registered recognizer routing ──────────────────────────────────

/// A trivial recognizer that claims any free text equal to `"matchme"` under
/// scope `app.demo` and emits a `Registered` payload.
struct DemoRecognizer;
impl InputScopeRecognizer for DemoRecognizer {
    fn scope(&self) -> InputScopeId {
        InputScopeId::new("app", "demo")
    }
    fn recognize(&self, input: &ResolvedInput) -> Option<InputIntentTarget> {
        match &input.kind {
            nmp_core::substrate::ResolvedInputKind::FreeText { text } if text == "matchme" => {
                Some(InputIntentTarget::Registered {
                    payload_json: r#"{"demo":true}"#.to_string(),
                })
            }
            _ => None,
        }
    }
    fn text_candidate(
        &self,
        free_text: &str,
        _targets: &TextSearchTargets,
    ) -> Option<InputIntentTarget> {
        (free_text == "matchme").then(|| InputIntentTarget::Registered {
            payload_json: r#"{"demo":true}"#.to_string(),
        })
    }
}

#[test]
fn dispatch_registered_returns_unrouted_candidate() {
    let app = test_app_new();
    // SAFETY: `test_app_new` never returns null.
    let app_ref = unsafe { &*app };
    let _ = app_ref.register_input_scope(Arc::new(DemoRecognizer) as Arc<dyn InputScopeRecognizer>);

    let (_, value) = dispatch(
        app,
        &request_json("matchme", &[("app", "demo")]),
        "sess-registered",
    );
    // Registered has no in-FFI seam; the candidate (scope + opaque payload) is
    // returned for the owning crate's handler to route.
    assert_eq!(value["dispatched"]["scope"]["name"], "demo");
    assert!(value["dispatched"]["target"]["Registered"]["payload_json"].is_string());
    test_app_free(app);
}

// ── D6: malformed input never NULL ───────────────────────────────────────────
