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
//! status) degrades silently to the mock store and surfaces
//! `MarmotInitError::KeyringUnavailable` in the snapshot (#1651).
//!
//! # V-62 contract (preserved)
//!
//! `register_with_keys` (in `ffi.rs`) reads the `Option<bool>` returned by
//! `initialize()`:
//! - `Some(false)` → real capability store, `init_error = None`.
//! - `Some(true)` → mock store (escape hatch or probe failure),
//!   `init_error = Some(MarmotInitError::KeyringUnavailable)` in the snapshot.
//! - `None` → store setup panicked; return null handle.
//!
//! # iOS ordering invariant (verified)
//!
//! `KernelModel.swift:266` runs `registerCapabilityHandler(…)` before
//! `KernelModel.swift:344` (`restoreChirpIdentity` via `start()`), which is
//! what drives the first `nmp_marmot_register*` call. This means the
//! capability slot is populated before `initialize()` runs its probe. Any
//! future reordering will cause the probe to fail → mock fallback →
//! `MarmotInitError::KeyringUnavailable` in the snapshot, which is visible to
//! the host. The ordering invariant is therefore self-enforcing.

use zeroize::Zeroizing;

use keyring_core::{
    Entry, Error as KeyringError, Result as KeyringResult,
    api::{CredentialApi, CredentialPersistence, CredentialStoreApi},
    set_default_store,
};
use nmp_core::{
    capability_socket::{CapabilityCallbackSlot, dispatch_capability},
    substrate::{
        CapabilityModule, KeyringCapability, KeyringIdentityWiring, KeyringRequest,
        KeyringResult as NmpKeyringResult, KeyringStatus,
    },
};
use std::{
    any::Any,
    sync::{Arc, OnceLock},
};

// ── Capability-backed CredentialStore ────────────────────────────────────────

/// `keyring-core` store that routes every operation through the host keyring
/// capability port. One `CapabilityCredentialStore` is installed as the
/// process-global default store once per `initialize()` call.
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
            payload_json: serde_json::to_string(&request).unwrap_or_else(|_| "{}".to_string()),
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
        // Wrap in `Zeroizing` so the base64 string is wiped from the heap when
        // this function returns (the key bytes must not linger in freed memory).
        let encoded = Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(secret));
        let result = self.dispatch(KeyringRequest::Store {
            account_id: self.account_id.clone(),
            secret: encoded.as_str().to_owned(),
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
                // Wrap the incoming string in `Zeroizing` so the base64-encoded
                // key bytes are wiped from the heap when we return.  The decoded
                // `Vec<u8>` is returned to mdk-sqlite-storage; the caller is
                // responsible for the lifetime of those bytes.
                let encoded: Zeroizing<String> = Zeroizing::new(result.secret.unwrap_or_default());
                base64::engine::general_purpose::STANDARD
                    .decode(encoded.as_str())
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
        // INVARIANT: `service` and `user` must remain slash-free literals so
        // the `"{service}/{user}"` join (in `build`) and this `split_once('/')`
        // round-trip stays injective. The only producers are mdk-sqlite-storage
        // (fixed literals) and our probe id (KEYRING_SERVICE_ID/KEYRING_DB_KEY_ID,
        // both slash-free) — so the invariant holds today.
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
/// The probe uses `keyring_service_id` + `KEYRING_DB_KEY_ID` as the
/// `account_id`, which is the SAME key `mdk-sqlite-storage` will look up in
/// production. This lets the probe test handler liveness without using a
/// synthetic key that could accidentally create a spurious keyring entry.
///
/// Returns `Some(false)` if the capability store is live (probe returned ok
/// or not_found), `Some(true)` if we degraded to the mock store.
fn try_install_capability_store(
    slot: CapabilityCallbackSlot,
    keyring_service_id: &str,
) -> Option<bool> {
    let store = Arc::new(CapabilityCredentialStore {
        slot: Arc::clone(&slot),
    });

    // Probe: one side-effect-free Retrieve using the caller-supplied
    // service-id and the generic DB-key id. Both must be slash-free so the
    // `"{service}/{user}"` join in `build` and the `split_once('/')` in
    // `get_specifiers` stay injective (see the INVARIANT comment there).
    let probe_id = format!("{}/{}", keyring_service_id, super::ffi::KEYRING_DB_KEY_ID);
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
        // Degrade to mock; register_with_keys surfaces KeyringUnavailable.
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
///
/// `keyring_service_id` is the app-scoped keyring namespace (e.g.
/// `"nmp.chirp.marmot"` for Chirp); it is forwarded to the probe so the
/// probe tests the REAL production account-id, not a synthetic one.
#[must_use]
pub(crate) fn initialize(slot: CapabilityCallbackSlot, keyring_service_id: &str) -> Option<bool> {
    // Escape hatch: if the caller has opted in via env var, install the
    // in-memory mock store unconditionally — before any platform check.
    // Off by default; production builds never set this variable.
    if env_requests_mock() {
        return install_mock_store();
    }

    try_install_capability_store(slot, keyring_service_id)
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
#[path = "credential_store_tests.rs"]
mod tests;
