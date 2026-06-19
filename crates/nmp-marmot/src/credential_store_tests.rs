//! Tests for the capability-backed credential store. (updated: keyring_service_id param)
//!
//! Split out of `credential_store.rs` (which was over the 500-LOC hard cap)
//! via `#[cfg(test)] #[path = "credential_store_tests.rs"] mod tests;`. The
//! module is still a child of `credential_store`, so `use super::*` resolves
//! to the (private) store/credential items under test.

use super::*;
use nmp_core::{
    capability_socket::{new_capability_callback_slot, CapabilityCallbackRegistration},
    substrate::{
        CapabilityEnvelope, CapabilityModule, KeyringCapability, KeyringRequest,
        KeyringResult as NmpKeyringResult,
    },
};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Mutex;

// Serialize tests that touch the process-global keyring default store or
// env vars so they do not race each other.
//
// Use `unwrap_or_else(|e| e.into_inner())` on every lock acquisition so that
// a panic inside one test does not poison the lock and cascade-fail all
// subsequent tests in the same process run.
static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

const VAR: &str = "NMP_MARMOT_MOCK_KEYRING";
/// App-scoped Marmot keyring service id used by the tests (arbitrary; just
/// needs to be non-empty so the probe account-id is well-formed).
const TEST_SVC: &str = "test.marmot.svc";

// In-memory KV store for the mock capability handler.  Shared by all tests
// that use `mock_keyring_handler`; each test reinitialises it to an empty
// HashMap before use.
static MOCK_KV: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

// ── env_requests_mock() parsing ──────────────────────────────────────────
//
// These tests only read/write env vars; they do not touch the global
// credential-store state, so they only need GLOBAL_LOCK (not MOCK_KV).

#[test]
fn env_mock_unset_is_false() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var(VAR);
    assert!(!env_requests_mock());
}

#[test]
fn env_mock_recognized_opt_in_values() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for val in ["1", "true", "True", "TRUE", "yes", "YES", " 1 ", " true "] {
        std::env::set_var(VAR, val);
        assert!(
            env_requests_mock(),
            "expected opt-in for NMP_MARMOT_MOCK_KEYRING={val:?}"
        );
    }
    std::env::remove_var(VAR);
}

#[test]
fn env_mock_non_opt_in_values_are_false() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for val in ["0", "false", "no", "off", "", "maybe", "2", "truetrue"] {
        std::env::set_var(VAR, val);
        assert!(
            !env_requests_mock(),
            "expected no opt-in for NMP_MARMOT_MOCK_KEYRING={val:?}"
        );
    }
    std::env::remove_var(VAR);
}

// ── initialize() with escape hatch active ────────────────────────────────

#[test]
fn initialize_returns_mock_when_env_set() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var(VAR, "1");
    let slot = new_capability_callback_slot();
    let result = initialize(slot, TEST_SVC);
    std::env::remove_var(VAR);
    assert_eq!(
        result,
        Some(true),
        "NMP_MARMOT_MOCK_KEYRING=1 must select the in-memory mock store on all platforms"
    );
}

// ── Mock capability handler (speaks the keyring vocabulary) ─────────────
//
// `extern "C"` functions cannot capture state, so the handler uses the
// process-static `MOCK_KV`.  Tests that use this handler must hold
// GLOBAL_LOCK and reset MOCK_KV to `Some(HashMap::new())` before running
// to avoid cross-test interference.

extern "C" fn mock_keyring_handler(
    _ctx: *mut c_void,
    request_json: *const c_char,
) -> *mut c_char {
    let json = unsafe { CStr::from_ptr(request_json) }
        .to_str()
        .unwrap_or("");
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let correlation_id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let payload = parsed
        .get("payload_json")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let result = match serde_json::from_str::<KeyringRequest>(payload) {
        Ok(KeyringRequest::Store { account_id, secret }) => {
            MOCK_KV
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashMap::new)
                .insert(account_id, secret);
            NmpKeyringResult::ok(None)
        }
        Ok(KeyringRequest::Retrieve { account_id }) => {
            match MOCK_KV
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashMap::new)
                .get(&account_id)
                .cloned()
            {
                Some(s) => NmpKeyringResult::ok(Some(s)),
                None => NmpKeyringResult::not_found(),
            }
        }
        Ok(KeyringRequest::Delete { account_id }) => {
            // Return not_found when the key is absent so that
            // CapabilityCredential::delete_credential can map it to
            // Error::NoEntry — matching the CredentialApi contract.
            let existed = MOCK_KV
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashMap::new)
                .remove(&account_id)
                .is_some();
            if existed {
                NmpKeyringResult::ok(None)
            } else {
                NmpKeyringResult::not_found()
            }
        }
        Err(_) => NmpKeyringResult::error(-50),
    };
    let envelope = CapabilityEnvelope {
        namespace: KeyringCapability::NAMESPACE.to_string(),
        correlation_id,
        result_json: serde_json::to_string(&result).unwrap(),
    };
    CString::new(serde_json::to_string(&envelope).unwrap())
        .unwrap()
        .into_raw()
}

/// Handler that always returns an error result (any op → error(-100)).
extern "C" fn error_handler(
    _ctx: *mut c_void,
    request_json: *const c_char,
) -> *mut c_char {
    let json = unsafe { CStr::from_ptr(request_json) }
        .to_str()
        .unwrap_or("");
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let correlation_id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let result = NmpKeyringResult::error(-100);
    let envelope = CapabilityEnvelope {
        namespace: KeyringCapability::NAMESPACE.to_string(),
        correlation_id,
        result_json: serde_json::to_string(&result).unwrap(),
    };
    CString::new(serde_json::to_string(&envelope).unwrap())
        .unwrap()
        .into_raw()
}

fn registered_slot_with(
    handler: nmp_core::capability_socket::CapabilityCallback,
) -> CapabilityCallbackSlot {
    let slot = new_capability_callback_slot();
    *slot.lock().unwrap() = Some(CapabilityCallbackRegistration {
        context: 0,
        callback: handler,
    });
    slot
}

fn empty_slot() -> CapabilityCallbackSlot {
    new_capability_callback_slot()
}

// ── CapabilityCredential mapping tests ───────────────────────────────────
//
// These tests exercise `CapabilityCredential` directly, without touching
// the process-global default store.  They still need GLOBAL_LOCK because
// they share MOCK_KV with initialize() probe tests.

/// `not_found` response from the handler maps to `Error::NoEntry`.
#[test]
fn not_found_maps_to_no_entry() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    *MOCK_KV.lock().unwrap_or_else(|e| e.into_inner()) = Some(HashMap::new());
    let slot = registered_slot_with(mock_keyring_handler);
    let cred = CapabilityCredential {
        slot,
        account_id: "svc/no-such-key".to_string(),
    };
    let err = cred.get_secret().unwrap_err();
    assert!(
        matches!(err, KeyringError::NoEntry),
        "not_found must map to NoEntry; got {err:?}"
    );
}

/// Explicit `error` status from the handler maps to `PlatformFailure`, never `NoEntry`.
///
/// This is the critical safety property: `mdk-sqlite-storage` uses `NoEntry`
/// to detect a DB file whose keyring entry is absent (→ `KeyringEntryMissingForExistingDatabase`).
/// Mapping capability errors to `NoEntry` would silently re-key an existing DB,
/// destroying all MLS group state.
#[test]
fn error_status_maps_to_platform_failure_not_no_entry() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let slot = registered_slot_with(error_handler);
    let cred = CapabilityCredential {
        slot,
        account_id: "svc/key".to_string(),
    };
    let err = cred.get_secret().unwrap_err();
    assert!(
        matches!(err, KeyringError::PlatformFailure(_)),
        "error status must map to PlatformFailure; got {err:?}"
    );
    assert!(
        !matches!(err, KeyringError::NoEntry),
        "error must NEVER map to NoEntry — that would silently re-key the DB"
    );
}

/// Missing handler (empty slot) maps to `PlatformFailure` (not `NoEntry`).
#[test]
fn missing_handler_maps_to_platform_failure() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let slot = empty_slot();
    let cred = CapabilityCredential {
        slot,
        account_id: "svc/key".to_string(),
    };
    let err = cred.get_secret().unwrap_err();
    assert!(
        matches!(err, KeyringError::PlatformFailure(_)),
        "missing handler must yield PlatformFailure; got {err:?}"
    );
}

/// set_secret → get_secret byte-identity round-trip via the mock handler.
///
/// Verifies the base64 encode/decode path is transparent.
#[test]
fn set_and_get_secret_round_trip() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    *MOCK_KV.lock().unwrap_or_else(|e| e.into_inner()) = Some(HashMap::new());
    let slot = registered_slot_with(mock_keyring_handler);
    let cred = CapabilityCredential {
        slot,
        account_id: "svc/db-key".to_string(),
    };
    let secret_bytes = b"super-secret-db-key-bytes-0123456789";
    cred.set_secret(secret_bytes).expect("set_secret must succeed");
    let retrieved = cred.get_secret().expect("get_secret must succeed");
    assert_eq!(retrieved, secret_bytes, "round-trip must be byte-identical");
}

/// `delete_credential` on a present key → Ok; on an absent key → NoEntry.
#[test]
fn delete_credential_present_returns_ok_absent_returns_no_entry() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    *MOCK_KV.lock().unwrap_or_else(|e| e.into_inner()) = Some(HashMap::new());
    let slot = registered_slot_with(mock_keyring_handler);
    let cred = CapabilityCredential {
        slot: Arc::clone(&slot),
        account_id: "svc/del-key".to_string(),
    };
    cred.set_secret(b"val").expect("set");
    cred.delete_credential()
        .expect("delete of present key must succeed");
    // Second delete — key is now absent → NoEntry.
    let err = cred.delete_credential().unwrap_err();
    assert!(
        matches!(err, KeyringError::NoEntry),
        "delete of absent key must return NoEntry; got {err:?}"
    );
}

// ── initialize() probe tests ─────────────────────────────────────────────
//
// These tests call `initialize()` which installs the process-global default
// store.  They MUST hold GLOBAL_LOCK and MUST unset NMP_MARMOT_MOCK_KEYRING
// before probing the capability path.
//
// NOTE: when the test binary is invoked with `NMP_MARMOT_MOCK_KEYRING=1`
// (e.g. to satisfy other test suites in the same crate), the env-set test
// still passes because it explicitly sets the var.  The capability-path tests
// (`initialize_with_live_handler_*`) correctly remove the var before calling
// `initialize()`, so they exercise the real probe branch even in that case.

/// When the capability handler returns ok/not_found, the probe succeeds
/// and `initialize()` returns `Some(false)` (real store chosen).
#[test]
fn initialize_with_live_handler_returns_some_false() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var(VAR);
    *MOCK_KV.lock().unwrap_or_else(|e| e.into_inner()) = Some(HashMap::new());
    let slot = registered_slot_with(mock_keyring_handler);
    let result = initialize(Arc::clone(&slot), TEST_SVC);
    assert_eq!(
        result,
        Some(false),
        "live handler → capability store → Some(false)"
    );
}

/// When the capability handler is missing, the probe fails and
/// `initialize()` degrades to the mock store (`Some(true)`).
#[test]
fn initialize_with_no_handler_degrades_to_mock() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var(VAR);
    let slot = empty_slot();
    let result = initialize(slot, TEST_SVC);
    assert_eq!(
        result,
        Some(true),
        "no handler → probe fails → mock store → Some(true)"
    );
}

/// When the capability handler always returns error, the probe fails and
/// `initialize()` degrades to the mock store (`Some(true)`).
#[test]
fn initialize_with_error_handler_degrades_to_mock() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var(VAR);
    let slot = registered_slot_with(error_handler);
    let result = initialize(slot, TEST_SVC);
    assert_eq!(
        result,
        Some(true),
        "error handler → probe fails → mock store → Some(true)"
    );
}
