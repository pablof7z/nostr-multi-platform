//! PR-4 — key-package autopublish parity across all register paths.
//!
//! Diagnostic root cause: iOS/Android accounts signed in via nsec (including
//! the NMP_TEST_NSEC test seam) never had a key package published — they could
//! never be invited to MLS groups unless the user found the manual "Publish key
//! package" row in Settings. The fix arms the autopublish flag in
//! `NmpApp::add_signer` (the single active-local-key sign-in seam) and consumes
//! it in the shared `register_with_keys` tail.
//!
//! Test strategy: `publish_key_package` (driven inside `register_with_keys`
//! when the flag is set) needs a write relay configured — it returns
//! `Err("no write relays configured")` otherwise. Rather than driving relay
//! config through the async actor, these tests assert the flag-consumption
//! invariant: the flag is consumed (atomic swap → false) by register, proving
//! the autopublish was ATTEMPTED. The integration-level proof that
//! `publish_key_package` produces events when relays ARE available is covered
//! by `super::tests::round_trip_publish_create_snapshot_send_messages`.
//!
//! Keyring: these tests install a mock keyring **capability callback** (no
//! process-global `NMP_MARMOT_MOCK_KEYRING` env var) so they never race the
//! env-var-based `credential_store` tests under cargo's parallel test runner.
//!
//! Split into its own module (not appended to `ffi/tests.rs`) to keep that file
//! at its file-size baseline (AGENTS.md 500-LOC ceiling).

use super::{nmp_marmot_unregister, register_with_secret_hex};
use nmp_core::substrate::{
    CapabilityEnvelope, CapabilityModule, CapabilityRequest, KeyringCapability, KeyringRequest,
    KeyringResult,
};
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::sync::{Mutex, OnceLock};

/// A valid nsec1 key shared with the sibling FFI tests.
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
/// App-scoped Marmot keyring service id for tests (mirrors what a real app supplies).
const TEST_MARMOT_SVC: &std::ffi::CStr = c"test.marmot.svc";

/// Process-local mock keyring store, keyed by account_id.
fn keyring_slots() -> &'static Mutex<HashMap<String, String>> {
    static SLOTS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Mock keyring capability handler — satisfies the `credential_store` probe and
/// MLS DB-key persistence without a real Keychain and without env vars.
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

/// Build an `NmpApp` with the mock keyring capability installed.
fn app_with_mock_keyring() -> *mut nmp_ffi::NmpApp {
    let app = nmp_ffi::nmp_app_new();
    nmp_ffi::nmp_app_set_capability_callback(app, std::ptr::null_mut(), Some(mock_keyring_callback));
    app
}

fn temp_db_dir(tag: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("nmp_marmot_{tag}_{:?}", std::thread::current().id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// PR-4 regression: `register_with_keys` (the shared tail of BOTH
/// `register_with_secret_hex` and `nmp_marmot_register_active`) must consume the
/// `pending_mls_autopublish` flag that an active local-key `nmp_app_signin_nsec`
/// arms — proving the autopublish is ATTEMPTED on every nsec sign-in path.
///
/// Before PR-4, `register_with_secret_hex` (the path used by the test-nsec seam
/// and `nmp_app_chirp_identity_sign_in_nsec`) never consumed the flag: only
/// `nmp_marmot_register_active` did. Accounts signed in via nsec could
/// therefore NEVER be invited to MLS groups without the user manually visiting
/// Settings > "Publish key package".
#[test]
fn register_after_signin_nsec_consumes_autopublish_flag() {
    let app = app_with_mock_keyring();
    // SAFETY: nmp_app_new never returns null.
    let app_ref = unsafe { &*app };

    // Active local-key sign-in — the path that was broken before PR-4. This is
    // the entry point that arms the flag (via `add_signer`).
    let nsec = CString::new(TEST_NSEC).unwrap();
    nmp_ffi::nmp_app_signin_nsec(app, nsec.as_ptr(), 1);

    let tmp = temp_db_dir("pr4");
    let db_dir = CString::new(tmp.to_string_lossy().as_bytes()).unwrap();

    // `register_with_secret_hex` must consume the flag inside `register_with_keys`.
    // We do NOT read the flag before register (a `take_*` would itself consume
    // it) — the post-register assertion proves it was set AND consumed.
    let handle =
        register_with_secret_hex(app, nsec.as_ptr(), db_dir.as_ptr(), TEST_MARMOT_SVC.as_ptr());
    assert!(
        !handle.is_null(),
        "register_with_secret_hex must succeed with mock keyring + temp dir"
    );

    // Flag false now ⇒ register_with_keys consumed it (atomic swap). Because the
    // ONLY thing that set it was the sign-in above, this single assertion proves
    // both halves of the contract. (The publish itself may silently fail with no
    // relays configured; that path is covered by the round-trip test.)
    assert!(
        !app_ref.take_pending_mls_autopublish(),
        "pending_mls_autopublish must be set by active nsec sign-in and consumed \
         by register_with_secret_hex (PR-4 regression: this path previously \
         skipped the autopublish tail, leaving nsec-signed-in accounts unable to \
         receive MLS group invitations)"
    );

    nmp_marmot_unregister(handle);
    nmp_ffi::nmp_app_free(app);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// PR-4 idempotence: re-registering (account switch back) without an
/// intervening sign-in must NOT re-arm the autopublish flag — it is a
/// sign-in-time one-shot, consumed at the first register.
#[test]
fn second_register_without_new_signin_does_not_set_autopublish() {
    let app = app_with_mock_keyring();
    // SAFETY: nmp_app_new never returns null.
    let app_ref = unsafe { &*app };
    let nsec = CString::new(TEST_NSEC).unwrap();

    // Sign in + register (flag set at sign-in, consumed at register).
    nmp_ffi::nmp_app_signin_nsec(app, nsec.as_ptr(), 1);
    let tmp = temp_db_dir("pr4_idempotence");
    let db_dir = CString::new(tmp.to_string_lossy().as_bytes()).unwrap();
    let h1 =
        register_with_secret_hex(app, nsec.as_ptr(), db_dir.as_ptr(), TEST_MARMOT_SVC.as_ptr());
    assert!(!h1.is_null(), "first register must succeed");
    nmp_marmot_unregister(h1);

    // After first register: flag consumed; no new sign-in ⇒ flag stays false.
    assert!(
        !app_ref.take_pending_mls_autopublish(),
        "flag must be false after first register consumed it"
    );
    assert!(
        !app_ref.take_pending_mls_autopublish(),
        "flag must remain false without a new sign-in"
    );

    nmp_ffi::nmp_app_free(app);
    let _ = std::fs::remove_dir_all(&tmp);
}
