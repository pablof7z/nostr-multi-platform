//! Integration test for [`nmp_app_template::register_defaults`].
//!
//! Spins up a real [`NmpApp`] via `nmp_app_new`, calls `register_defaults`,
//! and asserts that every canonical action namespace is reachable through
//! the standard FFI dispatch seam (`nmp_app_dispatch_action`). A registered
//! namespace round-trips a `correlation_id`; an unregistered namespace
//! comes back with an `error` field. That asymmetry is the lightest proof
//! the template actually wires what it claims to wire.

use std::ffi::{CStr, CString};

use nmp_ffi::{nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string, nmp_app_new};

/// All action namespaces [`nmp_app_template::register_defaults`] is
/// contracted to register.
const EXPECTED_NAMESPACES: &[&str] = &[
    // NIP-02 — substrate-level social graph (follow / unfollow / react).
    "nmp.follow",
    "nmp.unfollow",
    "nmp.nip25.react",
    // NIP-17 — DM send + DM-relay-list publish.
    "nmp.nip17.send",
    "nmp.nip17.publish_relay_list",
    // NIP-57 — lightning zap.
    "nmp.nip57.zap",
    // NIP-65 — relay-list publish (absorbed into nmp-router).
    "nmp.nip65.publish_relay_list",
];

#[test]
fn register_defaults_wires_every_canonical_namespace() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // SAFETY: `app` is a valid non-null pointer fresh from `nmp_app_new`.
    nmp_app_template::register_defaults(unsafe { &mut *app });

    for ns in EXPECTED_NAMESPACES {
        let result = dispatch(app, ns, "{}");
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("dispatch returned non-JSON");

        // A registered namespace either accepts (correlation_id) or
        // rejects on input-shape validation (error). The single failure
        // mode that proves NON-registration is "unknown namespace" — the
        // registry returns an error whose message contains the namespace
        // and the phrase "unknown". So: anything OTHER than
        // unknown-namespace counts as "registered".
        if let Some(err) = parsed.get("error").and_then(|e| e.as_str()) {
            assert!(
                !err.to_ascii_lowercase().contains("unknown"),
                "namespace `{ns}` was not registered by `register_defaults` \
                 (dispatch error: {err})"
            );
        }
        // If we got a correlation_id, registration is unambiguously proven.
    }

    // Confirm a genuinely-unregistered namespace surfaces the
    // unknown-namespace error — proves our above test is not vacuous.
    let bogus = dispatch(app, "nmp.template.never.registered", "{}");
    let parsed: serde_json::Value = serde_json::from_str(&bogus).expect("bogus reply not JSON");
    let err = parsed
        .get("error")
        .and_then(|e| e.as_str())
        .expect("unregistered namespace must surface an error");
    assert!(
        err.to_ascii_lowercase().contains("unknown"),
        "control case: expected unknown-namespace error, got: {err}"
    );

    nmp_app_free(app);
}

#[test]
fn register_defaults_is_repeatable_for_routing_and_runtime_slots() {
    // Composition root may legitimately re-run `register_defaults` (e.g.
    // a host that rebuilds its `NmpApp` factory). Action namespaces are
    // de-duplicated by the registry; routing-substrate / coverage-hook
    // slots are last-writer-wins; ingest parsers register additively (a
    // duplicate parser is harmless — the kernel calls all parsers for a
    // kind). The proof: a second call does not panic.
    let app = nmp_app_new();
    // SAFETY: same as above.
    nmp_app_template::register_defaults(unsafe { &mut *app });
    nmp_app_template::register_defaults(unsafe { &mut *app });
    nmp_app_free(app);
}

fn dispatch(app: *mut nmp_ffi::NmpApp, namespace: &str, action_json: &str) -> String {
    let ns_c = CString::new(namespace).unwrap();
    let json_c = CString::new(action_json).unwrap();
    let raw = nmp_app_dispatch_action(app, ns_c.as_ptr(), json_c.as_ptr());
    assert!(!raw.is_null(), "dispatch returned null for `{namespace}`");
    // SAFETY: `raw` is a fresh non-null C string owned by `nmp-core`.
    let s = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
    nmp_app_free_string(raw);
    s
}
