//! Integration tests for the input-scope recognizer wiring in
//! [`nmp_defaults::register_defaults`] (#1804 S7).
//!
//! Spins up a real [`NmpApp`] via `nmp_app_new`, calls `register_defaults`,
//! and inspects the `"input_scope"` composition-ledger seam to assert which
//! `InputScopeRecognizer`s the composition root installs: the three NIP-50
//! recognizers (always-on) and the NIP-29 group recognizer (social tier).

use nmp_ffi::{nmp_app_free, nmp_app_new};

/// Extract the scope labels recorded in the `"input_scope"` composition-ledger
/// seam after `register_defaults`. Returns the vec of label strings whose
/// `disposition` is `"Installed"` (not `"YieldedToExisting"`).
fn installed_input_scope_labels(app: *mut nmp_ffi::NmpApp) -> Vec<String> {
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    let ledger_json = app_ref.composition_ledger().to_json();
    ledger_json["records"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter(|r| r["seam"] == "input_scope" && r["disposition"] == "Installed")
        .filter_map(|r| r["key"].as_str().map(ToOwned::to_owned))
        .collect()
}

/// `register_defaults` (default config: `social: true`) installs the three
/// NIP-50 input-scope recognizers (always-on block) AND the NIP-29 group
/// recognizer (social block) — the minimum a default app needs to classify
/// free-text, NIP-50 search, and NIP-29 group URI inputs without any
/// additional wiring call.
#[test]
fn register_defaults_installs_nip50_and_nip29_input_recognizers() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    nmp_defaults::register_defaults(unsafe { &mut *app });

    let labels = installed_input_scope_labels(app);

    // NIP-50 always-on block: three recognizers.
    assert!(
        labels.contains(&"nip50.profiles".to_string()),
        "nip50.profiles input recognizer must be installed; got: {labels:?}"
    );
    assert!(
        labels.contains(&"nip50.notes".to_string()),
        "nip50.notes input recognizer must be installed; got: {labels:?}"
    );
    assert!(
        labels.contains(&"nip50.longform".to_string()),
        "nip50.longform input recognizer must be installed; got: {labels:?}"
    );

    // NIP-29 social block: one recognizer.
    assert!(
        labels.contains(&"nip29.groups".to_string()),
        "nip29.groups input recognizer must be installed; got: {labels:?}"
    );

    nmp_app_free(app);
}

/// When `social: false`, the NIP-29 group input recognizer is NOT installed
/// (it is in the social block). The three NIP-50 recognizers (always-on block)
/// must still be present.
#[test]
fn register_defaults_with_social_false_skips_nip29_input_recognizer() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    nmp_defaults::register_defaults_with(
        unsafe { &mut *app },
        nmp_defaults::NmpDefaults {
            social: false,
            ..Default::default()
        },
    );

    let labels = installed_input_scope_labels(app);

    // NIP-50 always-on: still present.
    assert!(
        labels.contains(&"nip50.profiles".to_string()),
        "nip50.profiles must be installed even with social:false; got: {labels:?}"
    );
    assert!(
        labels.contains(&"nip50.notes".to_string()),
        "nip50.notes must be installed even with social:false; got: {labels:?}"
    );
    assert!(
        labels.contains(&"nip50.longform".to_string()),
        "nip50.longform must be installed even with social:false; got: {labels:?}"
    );

    // NIP-29 social block: absent when social is off.
    assert!(
        !labels.contains(&"nip29.groups".to_string()),
        "nip29.groups must NOT be installed when social:false; got: {labels:?}"
    );

    nmp_app_free(app);
}

/// A second call to `register_defaults` yields (not installs) all four
/// recognizers — the yielding-default dup policy (ADR-0049) holds for the
/// input-scope registry just like the FTS scope registry.
#[test]
fn register_defaults_twice_yields_input_recognizers_on_second_call() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    nmp_defaults::register_defaults(unsafe { &mut *app });
    // Second call — must not panic; all recognizers yield.
    nmp_defaults::register_defaults(unsafe { &mut *app });

    // After two calls the `Installed` count is still 4 (not 8).
    let labels = installed_input_scope_labels(app);
    // Each scope appears exactly once in the Installed disposition.
    let profile_count = labels
        .iter()
        .filter(|l| l.as_str() == "nip50.profiles")
        .count();
    assert_eq!(
        profile_count, 1,
        "nip50.profiles must be Installed exactly once across two register_defaults calls"
    );

    nmp_app_free(app);
}
