//! Integration test: prove `register_actions` wires all three social-graph
//! namespaces against a real `NmpApp` and that each one round-trips through
//! `nmp_app_dispatch_action`.
//!
//! This is the migration-success contract — the same shape the chirp
//! `social_verbs_dispatch_through_action_registry` test enforces, lifted
//! into the substrate crate that now owns the modules.

use std::ffi::{CStr, CString};

use nmp_ffi::{nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string, nmp_app_new};

/// Drive `nmp_app_dispatch_action` for `namespace`/`action_json` and return
/// the parsed JSON result. The returned C string is freed.
fn dispatch(
    app: *mut nmp_ffi::NmpApp,
    namespace: &str,
    action_json: &str,
) -> serde_json::Value {
    let ns = CString::new(namespace).unwrap();
    let body = CString::new(action_json).unwrap();
    let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
    assert!(!ptr.is_null(), "dispatch_action must never return null");
    // SAFETY: `ptr` is a valid C string from `nmp_app_dispatch_action`.
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_app_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}

/// After `nmp_nip02::register_actions`, all three social verbs are
/// reachable through the generic `dispatch_action` path. Each accepted
/// dispatch returns a 32-hex `correlation_id`, proving BOTH the
/// shape-validating module (consumed by `ActionRegistry::start`) AND the
/// `ActorCommand`-enqueuing executor (consumed by `ActionRegistry::execute`)
/// are wired under each namespace.
#[test]
fn register_actions_wires_all_three_social_verbs() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new must return a valid app");
    // SAFETY: `app` is a valid pointer from `nmp_app_new`; we hold the
    // sole `&mut` for the duration of the registration call and drop it
    // before any other access.
    unsafe {
        nmp_nip02::register_actions(&mut *app);
    }

    for (namespace, body) in [
        ("nmp.follow", r#"{"pubkey":"deadbeef"}"#),
        ("nmp.unfollow", r#"{"pubkey":"deadbeef"}"#),
        ("nmp.nip25.react", r#"{"target_event_id":"abc","reaction":"+"}"#),
    ] {
        let parsed = dispatch(app, namespace, body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{namespace}: expected correlation_id, got {parsed}"));
        assert_eq!(
            id.len(),
            32,
            "{namespace}: correlation id must be 32 hex chars"
        );
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "{namespace}: correlation id must be lowercase hex, got {id}"
        );
    }

    // `nmp.nip25.react` accepts a body missing `reaction` (defaults to "+").
    let parsed = dispatch(app, "nmp.nip25.react", r#"{"target_event_id":"abc"}"#);
    assert!(
        parsed.get("correlation_id").is_some(),
        "nmp.nip25.react without `reaction` should default to '+' and succeed: {parsed}"
    );

    // Wrong-shape body is rejected by the module's shape validator (the
    // serde decoder), surfaced as `{"error":...}` (D6 — never a crash).
    let parsed = dispatch(app, "nmp.follow", r#"{"not_pubkey":"x"}"#);
    assert!(
        parsed.get("error").is_some(),
        "wrong-shape nmp.follow must be rejected: {parsed}"
    );

    nmp_app_free(app);
}

/// Unknown namespace is rejected by the registry — this proves the
/// registration is namespace-scoped (a host that calls `register_actions`
/// only gets the three social verbs, not a wildcard).
#[test]
fn unregistered_namespace_is_rejected_even_after_register_actions() {
    let app = nmp_app_new();
    // SAFETY: same as `register_actions_wires_all_three_social_verbs`.
    unsafe {
        nmp_nip02::register_actions(&mut *app);
    }
    let parsed = dispatch(app, "nmp.nip02.not_a_real_verb", r#"{}"#);
    assert!(
        parsed.get("error").is_some(),
        "unknown namespace must surface an error: {parsed}"
    );
    nmp_app_free(app);
}
