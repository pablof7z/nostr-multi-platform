//! Keyring-aware sign-in policy tests for `nmp-marmot::identity`.
//!
//! `nmp-marmot::identity` owns the two keyring-aware sign-in entry points
//! (`sign_in_nsec_with_keyring_account`, `restore_identity_with_keyring_account`)
//! relocated from `NmpApp` in issue #622. This module exercises the full
//! persist → recall → forget cycle through a mock keyring capability.
//!
//! Split out of `ffi/tests.rs` to keep that file under the 1000-LOC hard cap;
//! declared as a sibling `#[cfg(test)] mod keyring_identity_tests;` in `ffi.rs`,
//! the same way `autopublish_tests` / `deferred_kp_tests` are declared.

use nmp_core::substrate::{
    CapabilityEnvelope, CapabilityModule, CapabilityRequest, KeyringCapability, KeyringRequest,
    KeyringResult,
};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::sync::{Mutex, OnceLock};

// Keyed by account_id so concurrent actor-thread store operations (which use
// different account_ids like "nmp.identity.active.id") don't corrupt test state.
static KEYRING_SLOTS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn keyring_slots() -> &'static Mutex<HashMap<String, String>> {
    KEYRING_SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

extern "C" fn mock_keyring_callback(
    _context: *mut std::ffi::c_void,
    request_json: *const c_char,
) -> *mut c_char {
    let request = unsafe { CStr::from_ptr(request_json) }
        .to_str()
        .ok()
        .and_then(|s| serde_json::from_str::<CapabilityRequest>(s).ok());
    let result = match request {
        Some(req) if req.namespace == KeyringCapability::NAMESPACE => {
            match serde_json::from_str::<KeyringRequest>(&req.payload_json) {
                Ok(KeyringRequest::Store { account_id, secret }) => {
                    keyring_slots().lock().unwrap().insert(account_id, secret);
                    KeyringResult::ok(None)
                }
                Ok(KeyringRequest::Retrieve { account_id }) => {
                    match keyring_slots().lock().unwrap().get(&account_id).cloned() {
                        Some(secret) => KeyringResult::ok(Some(secret)),
                        None => KeyringResult::not_found(),
                    }
                }
                Ok(KeyringRequest::Delete { account_id }) => {
                    keyring_slots().lock().unwrap().remove(&account_id);
                    KeyringResult::ok(None)
                }
                Err(_) => KeyringResult::error(-50),
            }
        }
        _ => KeyringResult::error(-50),
    };
    let envelope = CapabilityEnvelope {
        namespace: KeyringCapability::NAMESPACE.to_string(),
        correlation_id: "test".to_string(),
        result_json: serde_json::to_string(&result).unwrap(),
    };
    CString::new(serde_json::to_string(&envelope).unwrap())
        .unwrap()
        .into_raw()
}

fn mock_keyring_json(request_json: String) -> String {
    let request = CString::new(request_json).expect("capability request has no interior NUL");
    let raw = mock_keyring_callback(std::ptr::null_mut(), request.as_ptr());
    if raw.is_null() {
        return "{}".to_string();
    }
    unsafe { CString::from_raw(raw) }
        .to_string_lossy()
        .into_owned()
}

// A valid nsec1 key shared with session_persistence_tests.
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

#[test]
fn nmp_marmot_identity_policy_owns_keyring_store_recall_forget() {
    // Verifies the full persist → recall → forget cycle through the two
    // `nmp-marmot::identity` entry points, which are the sole owners of
    // keyring-aware sign-in logic (relocated from `NmpApp` — issue #622).
    let app = nmp_ffi::nmp_app_new();
    unsafe { &*app }
        .capability_callback_slot()
        .set_native_handler(Some(std::sync::Arc::new(mock_keyring_json)));
    let app_ref = unsafe { &*app };

    // Persist via sign_in_nsec_with_keyring_account (returns null: no db_dir).
    let _ = crate::identity::sign_in_nsec_with_keyring_account(
        app,
        "test.keyring.acct",
        "test.marmot.svc",
        TEST_NSEC.to_string(),
        None,
    );
    // Recall via NmpApp::recall_local_nsec — the low-level primitive that
    // restore_identity_with_keyring_account delegates to.
    assert_eq!(
        app_ref.recall_local_nsec("test.keyring.acct").as_deref(),
        Some(TEST_NSEC)
    );
    app_ref.remove_account_forgetting_keyring("test.keyring.acct", "missing".to_string());
    assert_eq!(app_ref.recall_local_nsec("test.keyring.acct"), None);

    nmp_ffi::nmp_app_free(app);
}
