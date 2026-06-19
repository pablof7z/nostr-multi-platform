//! App-neutral identity/keyring helpers for Marmot host FFI wrappers.
//!
//! The reusable Marmot crate does not choose an app namespace, keyring account
//! id, or C symbol prefix. Host crates supply that app-owned keyring account id
//! and expose any per-app ABI wrappers they need.
//!
//! # Sign-in path ownership
//!
//! All sign-in paths ultimately call `NmpApp::add_signer` — the single
//! documented entry point in `nmp-ffi`. This module owns the two keyring-aware
//! variants that Marmot host shells need:
//!
//! - [`sign_in_nsec_with_keyring_account`] — **new account**: persists the
//!   secret to the host keyring, signs it into the kernel, and registers
//!   Marmot. Use this on first import or account creation.
//! - [`restore_identity_with_keyring_account`] — **returning user**: recalls
//!   the secret from the host keyring (or accepts an injected test secret),
//!   signs it in, and registers Marmot. Use this on app launch / session
//!   restore.
//!
//! Shells that do not embed Marmot use `nmp_app_signin_nsec` (C-ABI) directly
//! and are responsible for their own keyring management.

use nmp_core::substrate::KeyringIdentityWiring;
use nmp_ffi::NmpApp;
use nostr::Keys;
use zeroize::Zeroizing;

use crate::ffi::{register_with_keys, MarmotHandle};

fn sign_in_and_register_marmot(
    app: *mut NmpApp,
    secret: &str,
    db_dir: Option<&str>,
    keyring_service_id: &str,
) -> *mut MarmotHandle {
    let (Some(db_dir), Ok(keys)) = (db_dir, Keys::parse(secret)) else {
        return std::ptr::null_mut();
    };
    let db_path = format!("{}/marmot-mls-state.sqlite", db_dir.trim_end_matches('/'));
    register_with_keys(app, keys, &db_path, keyring_service_id)
}

/// Restore a caller-scoped local secret from the keyring, sign it into the
/// kernel actor, and register Marmot with the same account.
///
/// `keyring_account_id` is app-owned policy for the identity secret (e.g.
/// `"com.example.app.nsec"`). `keyring_service_id` is the app-scoped namespace
/// for the Marmot MLS DB encryption key (e.g. `"com.example.app.marmot"` —
/// distinct from the identity key so rotation does not collide). Both must be
/// non-empty; passing an empty id or a missing `db_dir` degrades gracefully to
/// a null Marmot handle (D6 — never panics on bad input).
///
/// When `test_nsec` is `Some`, that value is used directly instead of querying
/// the host keyring — for use in unit tests that do not wire a real keyring.
///
/// Internally: recalls the secret via `NmpApp::recall_local_nsec`, then calls
/// `NmpApp::add_signer(LocalNsec, make_active=true)`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn restore_identity_with_keyring_account(
    app: *mut NmpApp,
    keyring_account_id: &str,
    keyring_service_id: &str,
    db_dir: Option<&str>,
    test_nsec: Option<String>,
) -> *mut MarmotHandle {
    if app.is_null() || keyring_account_id.is_empty() || keyring_service_id.is_empty() {
        return std::ptr::null_mut();
    }
    let app_ref = unsafe { &*app };
    let secret = match test_nsec {
        Some(s) => Some(s),
        None => app_ref.recall_local_nsec(keyring_account_id),
    };
    let Some(secret) = secret else {
        return std::ptr::null_mut();
    };
    // `add_signer` arms MLS autopublish for this active local-key sign-in.
    app_ref.add_signer(
        nmp_core::SignerSource::LocalNsec(Zeroizing::new(secret.clone())),
        true,
    );
    sign_in_and_register_marmot(app, &secret, db_dir, keyring_service_id)
}

/// Persist a newly-imported local secret to the keyring, sign it into the
/// kernel actor, and register Marmot with the same account.
///
/// `keyring_account_id` is app-owned policy for the identity secret (e.g.
/// `"com.example.app.nsec"`). `keyring_service_id` is the app-scoped namespace
/// for the Marmot MLS DB encryption key (e.g. `"com.example.app.marmot"`).
/// Passing an empty id or a missing `db_dir` degrades gracefully to a null
/// Marmot handle (D6 — never panics on bad input).
///
/// Internally: stores the secret via `KeyringIdentityWiring::persist_secret`,
/// then calls `NmpApp::add_signer(LocalNsec, make_active=true)`.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn sign_in_nsec_with_keyring_account(
    app: *mut NmpApp,
    keyring_account_id: &str,
    keyring_service_id: &str,
    secret: String,
    db_dir: Option<&str>,
) -> *mut MarmotHandle {
    if app.is_null() || keyring_account_id.is_empty() || keyring_service_id.is_empty() {
        return std::ptr::null_mut();
    }
    let app_ref = unsafe { &*app };
    let req = KeyringIdentityWiring::persist_secret(
        "nmp.identity.persist",
        keyring_account_id,
        &secret,
    );
    let _ = app_ref.dispatch_capability(&req);
    // `add_signer` arms MLS autopublish for this active local-key sign-in.
    app_ref.add_signer(
        nmp_core::SignerSource::LocalNsec(Zeroizing::new(secret.clone())),
        true,
    );
    sign_in_and_register_marmot(app, &secret, db_dir, keyring_service_id)
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
            "example.marmot.svc",
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
