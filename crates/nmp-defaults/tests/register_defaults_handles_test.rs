//! Handle-returning default composition tests.

use nmp_ffi::{nmp_app_free, nmp_app_new};

#[test]
fn register_defaults_with_handles_returns_mute_runtime_when_social_is_enabled() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    let handles = nmp_defaults::register_defaults_with_handles(
        unsafe { &mut *app },
        nmp_defaults::NmpDefaults::default(),
    );

    assert!(
        handles.mute.is_some(),
        "default social composition must return the installed mute runtime handle"
    );
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    assert!(
        app_ref
            .registered_typed_projection_keys()
            .contains(&"nmp.nip51.mute_list".to_string()),
        "handle-returning entry point must preserve mute-list projection registration"
    );

    nmp_app_free(app);
}

#[test]
fn register_defaults_with_handles_omits_mute_runtime_when_social_is_disabled() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    let handles = nmp_defaults::register_defaults_with_handles(
        unsafe { &mut *app },
        nmp_defaults::NmpDefaults {
            social: false,
            ..Default::default()
        },
    );

    assert!(
        handles.mute.is_none(),
        "social:false must not install or return the mute runtime handle"
    );
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };
    assert!(
        !app_ref
            .registered_typed_projection_keys()
            .contains(&"nmp.nip51.mute_list".to_string()),
        "social:false must not register the mute-list projection"
    );

    nmp_app_free(app);
}

#[test]
fn register_defaults_with_handles_uses_empty_search_defaults_by_default() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    let handles = nmp_defaults::register_defaults_with_handles(
        unsafe { &mut *app },
        nmp_defaults::NmpDefaults::default(),
    );
    let search_relays = handles
        .search_relays
        .as_ref()
        .expect("default social composition must wire search relays");

    assert!(
        nmp_defaults::effective_search_relays(
            search_relays,
            &nmp_defaults::SearchDefaults::default()
        )
        .is_empty(),
        "shared defaults must not install an operator search relay"
    );

    nmp_app_free(app);
}

#[test]
fn register_defaults_with_handles_accepts_app_search_defaults() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    let search_defaults = nmp_defaults::SearchDefaults::with_default_relays(vec![
        "wss://app-search.example".to_string(),
    ]);
    let handles = nmp_defaults::register_defaults_with_handles(
        unsafe { &mut *app },
        nmp_defaults::NmpDefaults {
            search_defaults: search_defaults.clone(),
            ..Default::default()
        },
    );
    let search_relays = handles
        .search_relays
        .as_ref()
        .expect("default social composition must wire search relays");

    assert_eq!(
        nmp_defaults::effective_search_relays(search_relays, &search_defaults),
        vec!["wss://app-search.example".to_string()],
        "app-supplied search defaults must be the fallback after missing kind:10007"
    );

    nmp_app_free(app);
}
