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
