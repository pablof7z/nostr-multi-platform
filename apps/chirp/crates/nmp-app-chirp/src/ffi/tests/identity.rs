//! Chirp identity wrapper regressions.

use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::sync::{Mutex, OnceLock};

use nmp_core::substrate::{
    CapabilityEnvelope, CapabilityModule, CapabilityRequest, KeyringCapability, KeyringRequest,
    KeyringResult,
};
use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_app_set_capability_callback};

use super::super::nmp_app_chirp_identity_sign_in_nsec;

const LEGACY_CHIRP_MARMOT_SLOT: &str = "chirp.marmot.cached_secret";
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

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

#[test]
fn marmot_sign_in_does_not_write_legacy_fixed_keyring_slot() {
    keyring_slots().lock().unwrap().clear();
    let app = nmp_app_new();
    nmp_app_set_capability_callback(app, std::ptr::null_mut(), Some(mock_keyring_callback));

    let secret = CString::new(TEST_NSEC).unwrap();
    let handle = nmp_app_chirp_identity_sign_in_nsec(app, secret.as_ptr(), std::ptr::null());
    assert!(
        handle.is_null(),
        "missing db dir should not register Marmot"
    );
    assert!(
        !keyring_slots()
            .lock()
            .unwrap()
            .contains_key(LEGACY_CHIRP_MARMOT_SLOT),
        "Marmot sign-in must use actor-owned session persistence, not the legacy fixed slot"
    );

    nmp_app_free(app);
}
