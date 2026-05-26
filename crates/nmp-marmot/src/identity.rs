//! App-neutral identity/keyring helpers for Marmot host FFI wrappers.
//!
//! The reusable Marmot crate does not choose an app namespace, keyring account
//! id, or C symbol prefix. Host crates supply that app-owned keyring account id
//! and expose any per-app ABI wrappers they need.

use nmp_ffi::NmpApp;
use nostr::Keys;

use crate::ffi::{register_with_keys, MarmotHandle};

fn sign_in_and_register_marmot(
    app: *mut NmpApp,
    secret: &str,
    db_dir: Option<&str>,
) -> *mut MarmotHandle {
    let (Some(db_dir), Ok(keys)) = (db_dir, Keys::parse(secret)) else {
        return std::ptr::null_mut();
    };
    let db_path = format!("{}/marmot-mls-state.sqlite", db_dir.trim_end_matches('/'));
    register_with_keys(app, keys, &db_path)
}

/// Restore a caller-scoped local secret, sign it into the kernel actor, and
/// register Marmot with the same account.
///
/// `keyring_account_id` is app-owned policy. Passing an empty id or missing
/// `db_dir` degrades to a null Marmot handle.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn restore_identity_with_keyring_account(
    app: *mut NmpApp,
    keyring_account_id: &str,
    db_dir: Option<&str>,
    test_nsec: Option<String>,
) -> *mut MarmotHandle {
    if app.is_null() || keyring_account_id.is_empty() {
        return std::ptr::null_mut();
    }
    let app_ref = unsafe { &*app };
    let secret = app_ref.restore_local_nsec_from_keyring(keyring_account_id, test_nsec);
    let Some(secret) = secret else {
        return std::ptr::null_mut();
    };
    sign_in_and_register_marmot(app, &secret, db_dir)
}

/// Persist a caller-scoped local secret, sign it into the kernel actor, and
/// register Marmot with the same account.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn sign_in_nsec_with_keyring_account(
    app: *mut NmpApp,
    keyring_account_id: &str,
    secret: String,
    db_dir: Option<&str>,
) -> *mut MarmotHandle {
    if app.is_null() || keyring_account_id.is_empty() {
        return std::ptr::null_mut();
    }
    let app_ref = unsafe { &*app };
    let secret = app_ref.sign_in_local_nsec_with_keyring(keyring_account_id, secret);
    sign_in_and_register_marmot(app, &secret, db_dir)
}

/// Forget a caller-scoped local secret and remove the identity through the
/// kernel actor.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn remove_identity_with_keyring_account(
    app: *mut NmpApp,
    keyring_account_id: &str,
    identity_id: String,
) {
    if app.is_null() || keyring_account_id.is_empty() {
        return;
    }
    let app_ref = unsafe { &*app };
    app_ref.remove_account_forgetting_keyring(keyring_account_id, identity_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::{
        CapabilityEnvelope, CapabilityModule, CapabilityRequest, KeyringCapability, KeyringRequest,
        KeyringResult,
    };
    use std::collections::HashMap;
    use std::ffi::{c_char, CStr, CString};
    use std::sync::{Mutex, OnceLock};

    const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

    static KEYRING_SLOTS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

    fn keyring_slots() -> &'static Mutex<HashMap<String, String>> {
        KEYRING_SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn lock_keyring_slots() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
        keyring_slots()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
                        lock_keyring_slots().insert(account_id, secret);
                        KeyringResult::ok(None)
                    }
                    Ok(KeyringRequest::Retrieve { account_id }) => {
                        match lock_keyring_slots().get(&account_id).cloned() {
                            Some(secret) => KeyringResult::ok(Some(secret)),
                            None => KeyringResult::not_found(),
                        }
                    }
                    Ok(KeyringRequest::Delete { account_id }) => {
                        lock_keyring_slots().remove(&account_id);
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

    fn new_app_with_keyring() -> *mut NmpApp {
        let app = nmp_ffi::nmp_app_new();
        nmp_ffi::nmp_app_set_capability_callback(
            app,
            std::ptr::null_mut(),
            Some(mock_keyring_callback),
        );
        app
    }

    #[test]
    fn sign_in_uses_caller_supplied_keyring_account() {
        let app = new_app_with_keyring();
        let handle = sign_in_nsec_with_keyring_account(
            app,
            "example.marmot.sign_in.local_secret",
            TEST_NSEC.to_string(),
            None,
        );

        assert!(
            handle.is_null(),
            "missing db dir should not register Marmot"
        );
        let slots = lock_keyring_slots();
        assert_eq!(
            slots
                .get("example.marmot.sign_in.local_secret")
                .map(String::as_str),
            Some(TEST_NSEC)
        );
        drop(slots);

        nmp_ffi::nmp_app_free(app);
    }

    #[test]
    fn remove_uses_caller_supplied_keyring_account() {
        lock_keyring_slots().insert(
            "example.marmot.remove.local_secret".to_string(),
            TEST_NSEC.to_string(),
        );
        let app = new_app_with_keyring();

        remove_identity_with_keyring_account(
            app,
            "example.marmot.remove.local_secret",
            "missing".to_string(),
        );

        let slots = lock_keyring_slots();
        assert!(!slots.contains_key("example.marmot.remove.local_secret"));
        drop(slots);

        nmp_ffi::nmp_app_free(app);
    }
}
