use std::ffi::CString;

use crate::resolve_ref::{
    nmp_app_release_profile_ref, nmp_app_resolve_event_embed_with_metadata,
    nmp_app_resolve_profile_ref,
};
use crate::{nmp_app_free, nmp_app_new};

const PROFILE_KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EVENT_KEY: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[test]
fn typed_profile_adapters_reject_malformed_keys_before_enqueue() {
    let app = nmp_app_new();
    let app_ref = unsafe { &*app };
    let valid = CString::new(PROFILE_KEY).unwrap();
    let invalid = CString::new("not-a-profile-key").unwrap();
    let consumer = CString::new("ffi-test-profile").unwrap();

    nmp_app_resolve_profile_ref(app, valid.as_ptr(), consumer.as_ptr());
    assert_eq!(app_ref.send_cmd_count_for_test(), 1);

    nmp_app_resolve_profile_ref(app, invalid.as_ptr(), consumer.as_ptr());
    nmp_app_release_profile_ref(app, invalid.as_ptr(), consumer.as_ptr());
    assert_eq!(
        app_ref.send_cmd_count_for_test(),
        1,
        "malformed profile keys must fail closed before actor enqueue"
    );

    nmp_app_release_profile_ref(app, valid.as_ptr(), consumer.as_ptr());
    assert_eq!(app_ref.send_cmd_count_for_test(), 2);

    nmp_app_free(app);
}

#[test]
fn typed_event_metadata_adapter_rejects_malformed_metadata_before_enqueue() {
    let app = nmp_app_new();
    let app_ref = unsafe { &*app };
    let key = CString::new(EVENT_KEY).unwrap();
    let consumer = CString::new("ffi-test-event").unwrap();
    let malformed = CString::new(r#"{"hints":[42]}"#).unwrap();
    let metadata = CString::new(r#"{"hints":["wss://relay.example"],"kind":1}"#).unwrap();

    nmp_app_resolve_event_embed_with_metadata(
        app,
        key.as_ptr(),
        consumer.as_ptr(),
        malformed.as_ptr(),
    );
    assert_eq!(
        app_ref.send_cmd_count_for_test(),
        0,
        "malformed metadata must fail closed before actor enqueue"
    );

    nmp_app_resolve_event_embed_with_metadata(
        app,
        key.as_ptr(),
        consumer.as_ptr(),
        metadata.as_ptr(),
    );
    assert_eq!(app_ref.send_cmd_count_for_test(), 1);

    nmp_app_free(app);
}
