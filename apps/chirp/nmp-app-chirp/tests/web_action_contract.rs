use std::ffi::{CStr, CString};

use nmp_app_chirp::{nmp_app_chirp_register, nmp_app_chirp_unregister};
use nmp_core::{nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string, nmp_app_new, NmpApp};
use nmp_wasm::{ChirpAction, ChirpActionDispatch};

fn dispatch(app: *mut NmpApp, namespace: &str, action_json: &str) -> serde_json::Value {
    let ns = CString::new(namespace).unwrap();
    let body = CString::new(action_json).unwrap();
    let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
    assert!(!ptr.is_null(), "dispatch_action must never return null");
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_app_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}

#[test]
fn web_chirp_action_contract_dispatches_against_registered_app_actions() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null());

    for (intent, expected_namespace) in [
        (
            ChirpAction::React {
                target_event_id: "abc".to_string(),
                reaction: "+".to_string(),
            },
            "chirp.react",
        ),
        (
            ChirpAction::Follow {
                pubkey: "deadbeef".to_string(),
            },
            "chirp.follow",
        ),
        (
            ChirpAction::Unfollow {
                pubkey: "deadbeef".to_string(),
            },
            "chirp.unfollow",
        ),
    ] {
        let action = ChirpActionDispatch {
            action: intent,
            correlation_id: "web-contract".to_string(),
        }
        .into_action_dispatch();
        assert_eq!(action.action_type, expected_namespace);

        let body = action.payload.to_string();
        let parsed = dispatch(app, &action.action_type, &body);
        assert!(
            parsed.get("correlation_id").is_some(),
            "web Chirp contract must dispatch through app action registry: {parsed}"
        );
    }

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
