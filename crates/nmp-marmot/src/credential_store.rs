//! Platform credential-store setup for Marmot SQLite encryption.
//!
//! # Unified capability-backed keyring store
//!
//! All platforms (iOS, macOS, Android, Linux, WASM) now use a single
//! `CapabilityCredentialStore` that routes `keyring-core` calls through the
//! host keyring capability — the same seam `nmp-ffi::NmpApp::dispatch_capability`
//! uses. iOS wires `KeychainCapability.handleJSON(_:)` into that seam before
//! calling the first `nmp_marmot_register*`; Android wires its Keystore
//! implementation. This eliminates the old `apple-native-keyring-store`
//! dependency and, critically, fixes the Android durability bug: previously
//! the MLS SQLite DB key lived only in process memory, so group secrets were
//! lost on every app restart.
//!
//! # Headless / CI escape hatch
//!
//! Setting the environment variable `NMP_MARMOT_MOCK_KEYRING=1` (or `true`,
//! case-insensitive) before the process starts causes `initialize()` to
//! install the in-memory mock store **on every platform**, bypassing the
//! capability probe entirely.
//!
//! This is an **opt-in testability seam only**. Production iOS and macOS
//! builds never set this variable, so their behaviour is completely
//! unchanged. The mock store is ephemeral (process-local, no persistence),
//! which makes it unsuitable for production use but ideal for headless CI,
//! integration harnesses, and the `chirp-repl` MLS round-trip smoke test.
//!
//! Example:
//! ```text
//! NMP_MARMOT_MOCK_KEYRING=1 cargo test -p nmp-marmot --features ffi
//! ```
//!
//! # Probe strategy
//!
//! When the env escape hatch is not set, `initialize()` builds a
//! `CapabilityCredentialStore` and issues one side-effect-free `Retrieve`
//! probe for the MLS DB key id. A decodable `ok` or `not_found` response
//! confirms the handler is alive and speaks the keyring vocabulary; any
//! other outcome (undecodable envelope, missing handler, explicit `error`
//! status) degrades silently to the mock store and sets `keyring_unavailable`.
//!
//! # V-62 contract (preserved)
//!
//! `register_with_keys` (in `ffi.rs`) reads the `Option<bool>` returned by
//! `initialize()`:
//! - `Some(false)` → real capability store, `keyring_unavailable = false`.
//! - `Some(true)` → mock store (escape hatch or probe failure),
//!   `keyring_unavailable = true` in the snapshot.
//! - `None` → store setup panicked; return null handle.
//!
//! # iOS ordering invariant (verified)
//!
//! `KernelBridge.swift:76` runs `registerCapabilityHandler(…)` before the
//! first `restoreChirpIdentity` / `nmp_marmot_register*` call. This means
//! the capability slot is populated before `initialize()` runs its probe.
//! Any future reordering will cause the probe to fail → mock fallback →
//! `keyring_unavailable = true` in the snapshot, which is visible to the
//! host. The ordering invariant is therefore self-enforcing.

use keyring_core::{
    api::{CredentialApi, CredentialPersistence, CredentialStoreApi},
    set_default_store,
    Entry, Error as KeyringError, Result as KeyringResult,
};
use nmp_core::{
    capability_socket::{dispatch_capability, CapabilityCallbackSlot},
    substrate::{
        CapabilityModule, KeyringCapability, KeyringIdentityWiring, KeyringRequest, KeyringResult as NmpKeyringResult, KeyringStatus,
    },
};
use std::{
    any::Any,
    sync::{Arc, OnceLock},
};

// ── Capability-backed CredentialStore ────────────────────────────────────────

/// `keyring-core` store that routes every operation through the host keyring
/// capability port (the same seam `nmp_app_set_capability_callback` plugs
/// into). One `CapabilityCredentialStore` is installed as the process-global
/// default store once per `initialize()` call.
struct CapabilityCredentialStore {
    slot: CapabilityCallbackSlot,
}

impl CredentialStoreApi for CapabilityCredentialStore {
    fn vendor(&self) -> String {
        "nmp-marmot/capability-keyring-store".to_string()
    }

    fn id(&self) -> String {
        "v1".to_string()
    }

    fn build(
        &self,
        service: &str,
        user: &str,
        _modifiers: Option<&std::collections::HashMap<&str, &str>>,
    ) -> KeyringResult<Entry> {
        let credential = Arc::new(CapabilityCredential {
            slot: Arc::clone(&self.slot),
            // Wire-stable account_id: "<service>/<user>".  mdk-sqlite-storage
            // uses fixed literals for both fields, so this is stable across
            // versions as long as those literals don't change.
            account_id: format!("{service}/{user}"),
        });
        Ok(Entry::new_with_credential(credential))
    }

    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::UntilDelete
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ── Capability-backed Credential ─────────────────────────────────────────────

struct CapabilityCredential {
    slot: CapabilityCallbackSlot,
    /// Wire-stable key: `"<service>/<user>"`.
    account_id: String,
}

impl CapabilityCredential {
    /// Dispatch a keyring request and decode the result.
    ///
    /// Returns `KeyringStatus::NotFound` only for explicit `not_found` in the
    /// result; any other non-ok outcome is `Error` (see module doc for why
    /// this matters for `mdk`'s `KeyringEntryMissingForExistingDatabase`).
    fn dispatch(&self, request: KeyringRequest) -> NmpKeyringResult {
        let correlation_id = format!("marmot-{}", uuid_correlation());
        let cap_req = nmp_core::substrate::CapabilityRequest {
            namespace: KeyringCapability::NAMESPACE.to_string(),
            correlation_id,
            payload_json: serde_json::to_string(&request)
                .unwrap_or_else(|_| "{}".to_string()),
        };
        let json = serde_json::to_string(&cap_req).unwrap_or_else(|_| "{}".to_string());
        let envelope_json = dispatch_capability(&self.slot, &json);

        // Parse the envelope; a malformed response is reported as error (D6).
        match serde_json::from_str::<nmp_core::substrate::CapabilityEnvelope>(&envelope_json) {
            Ok(envelope) => KeyringIdentityWiring::decode_result(&envelope),
            Err(_) => NmpKeyringResult::error(-50),
        }
    }
}

/// Simple monotonic counter for correlation IDs (process-unique, not globally unique).
fn uuid_correlation() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

impl CredentialApi for CapabilityCredential {
    fn set_secret(&self, secret: &[u8]) -> KeyringResult<()> {
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(secret);
        let result = self.dispatch(KeyringRequest::Store {
            account_id: self.account_id.clone(),
            secret: encoded,
        });
        match result.status {
            KeyringStatus::Ok => Ok(()),
            KeyringStatus::NotFound => Err(KeyringError::NoEntry),
            KeyringStatus::Error => Err(platform_failure(result.os_status)),
        }
    }

    fn get_secret(&self) -> KeyringResult<Vec<u8>> {
        use base64::Engine as _;
        let result = self.dispatch(KeyringRequest::Retrieve {
            account_id: self.account_id.clone(),
        });
        match result.status {
            KeyringStatus::Ok => {
                let encoded = result.secret.as_deref().unwrap_or("");
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|_| platform_failure(Some(-50)))
            }
            // ONLY explicit not_found maps to NoEntry.  This is critical:
            // mdk-sqlite-storage detects an existing DB file with no keyring
            // entry and raises `KeyringEntryMissingForExistingDatabase` when
            // it sees `Error::NoEntry`.  Any undecodable / error response
            // must therefore be PlatformFailure, never NoEntry, so mdk never
            // silently re-keys an existing DB.
            KeyringStatus::NotFound => Err(KeyringError::NoEntry),
            KeyringStatus::Error => Err(platform_failure(result.os_status)),
        }
    }

    fn delete_credential(&self) -> KeyringResult<()> {
        let result = self.dispatch(KeyringRequest::Delete {
            account_id: self.account_id.clone(),
        });
        match result.status {
            KeyringStatus::Ok => Ok(()),
            KeyringStatus::NotFound => Err(KeyringError::NoEntry),
            KeyringStatus::Error => Err(platform_failure(result.os_status)),
        }
    }

    fn get_credential(&self) -> KeyringResult<Option<Arc<keyring_core::api::Credential>>> {
        // This store does not support credential wrapping; return None to
        // let `keyring_core::Entry::get_credential` hand back `self`.
        Ok(None)
    }

    fn get_specifiers(&self) -> Option<(String, String)> {
        // account_id is "<service>/<user>" — split on the first '/'.
        let (svc, usr) = self.account_id.split_once('/')?;
        Some((svc.to_string(), usr.to_string()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn platform_failure(os_status: Option<i32>) -> KeyringError {
    let code = os_status.unwrap_or(-50);
    KeyringError::PlatformFailure(format!("capability keyring error (os_status={code})").into())
}

// ── Policy entrypoints ───────────────────────────────────────────────────────

/// Returns `true` if the env var `NMP_MARMOT_MOCK_KEYRING` is set to an
/// opt-in value (`1`, `true`, `yes` — case-insensitive). Any other value
/// (including unset, empty, or malformed) is treated as `false` (D6: no
/// panics on bad input).
fn env_requests_mock() -> bool {
    match std::env::var("NMP_MARMOT_MOCK_KEYRING") {
        Ok(val) => matches!(val.trim().to_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => false,
    }
}

/// Install the capability-backed credential store and probe it with one
/// side-effect-free `Retrieve`.
///
/// Returns `Some(false)` if the capability store is live (probe returned ok
/// or not_found), `Some(true)` if we degraded to the mock store.
fn try_install_capability_store(slot: CapabilityCallbackSlot) -> Option<bool> {
    let store = Arc::new(CapabilityCredentialStore {
        slot: Arc::clone(&slot),
    });

    // Probe: one side-effect-free Retrieve of the DB-key account_id.
    // mdk-sqlite-storage uses KEYRING_SERVICE_ID + KEYRING_DB_KEY_ID as the
    // service/user pair (see ffi.rs); the probe uses the same derived id.
    let probe_id = format!("{}/{}", super::ffi::KEYRING_SERVICE_ID, super::ffi::KEYRING_DB_KEY_ID);
    let credential = CapabilityCredential {
        slot: Arc::clone(&slot),
        account_id: probe_id,
    };
    let probe_result = credential.dispatch(KeyringRequest::Retrieve {
        account_id: credential.account_id.clone(),
    });

    let handler_alive = matches!(
        probe_result.status,
        KeyringStatus::Ok | KeyringStatus::NotFound
    );

    if handler_alive {
        set_default_store(store);
        Some(false)
    } else {
        // Handler missing, returned error, or envelope was malformed.
        // Degrade to mock; register_with_keys will set keyring_unavailable.
        install_mock_store()
    }
}

/// Install the process-global default store and return `Some(use_mock)`.
///
/// Policy (V-62, no `cfg(target_os)` branches remain):
///
/// a. `NMP_MARMOT_MOCK_KEYRING` set → mock store, `Some(true)` (unchanged).
/// b. Build `CapabilityCredentialStore`; probe with one side-effect-free
///    `Retrieve` of the DB-key id:
///    - decodable ok/not_found → capability store, `Some(false)`.
///    - anything else → mock + `Some(true)`.
#[must_use]
pub(crate) fn initialize(slot: CapabilityCallbackSlot) -> Option<bool> {
    // Escape hatch: if the caller has opted in via env var, install the
    // in-memory mock store unconditionally — before any platform check.
    // Off by default; production builds never set this variable.
    if env_requests_mock() {
        return install_mock_store();
    }

    try_install_capability_store(slot)
}

#[must_use]
pub(crate) fn install_mock_store() -> Option<bool> {
    static MOCK_STORE: OnceLock<Arc<keyring_core::api::CredentialStore>> = OnceLock::new();
    let store = Arc::clone(MOCK_STORE.get_or_init(|| {
        let store: Arc<keyring_core::api::CredentialStore> =
            keyring_core::mock::Store::new().expect("mock keyring store");
        store
    }));
    set_default_store(store);
    Some(true)
}

#[cfg(test)]
mod tests {
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
        let result = initialize(slot);
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
        let result = initialize(Arc::clone(&slot));
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
        let result = initialize(slot);
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
        let result = initialize(slot);
        assert_eq!(
            result,
            Some(true),
            "error handler → probe fails → mock store → Some(true)"
        );
    }
}
