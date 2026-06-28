//! Host-side keyring capability handler for chirp-desktop.
//!
//! Wires the NMP `KeyringCapability` socket to the OS credential store. The
//! shell only executes store/retrieve/delete and reports raw results; identity
//! policy remains in Rust. A read/delete path for the old plaintext session
//! directory is retained only to migrate or remove existing files.

use std::ffi::{c_char, c_void, CStr, CString};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use zeroize::Zeroizing;

use nmp_core::substrate::{
    CapabilityModule, CapabilityRequest, KeyringCapability, KeyringRequest, KeyringStatus,
};

const KEYRING_SERVICE: &str = "com.nmp.chirp-desktop.session";
const ERR_PLATFORM: i32 = -1;
const ERR_MALFORMED: i32 = -50;

pub(crate) fn chirp_data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/chirp-desktop"))
    } else if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        Some(PathBuf::from(xdg).join("chirp-desktop"))
    } else {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(".local/share/chirp-desktop"))
    }
}

/// Legacy directory that held one plaintext secret file per account.
fn legacy_sessions_dir() -> Option<PathBuf> {
    chirp_data_dir().map(|d| d.join("sessions"))
}

/// Path to the legacy secret file for `account_id` under `base`.
///
/// `account_id` is a nostr pubkey (hex), which is already path-safe. We still
/// reject any value containing a path separator or NUL to avoid traversal.
fn legacy_session_path_in(base: &Path, account_id: &str) -> Option<PathBuf> {
    if account_id.is_empty()
        || account_id.contains('/')
        || account_id.contains('\\')
        || account_id.contains('\0')
    {
        return None;
    }
    Some(base.join(account_id))
}

pub(crate) extern "C" fn keyring_handler(
    _ctx: *mut c_void,
    request_json: *const c_char,
) -> *mut c_char {
    let envelope_json = build_envelope_json(request_json);
    CString::new(envelope_json.as_bytes())
        .unwrap_or_else(|_| CString::new("{}").expect("static literal has no NUL"))
        .into_raw()
}

fn build_envelope_json(request_json: *const c_char) -> Zeroizing<String> {
    if request_json.is_null() {
        return error_envelope("", "");
    }
    let request_str = match unsafe { CStr::from_ptr(request_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return error_envelope("", ""),
    };
    let parsed: CapabilityRequest = match serde_json::from_str(request_str) {
        Ok(req) => req,
        Err(_) => return error_envelope("", ""),
    };

    if parsed.namespace != KeyringCapability::NAMESPACE {
        return error_envelope(&parsed.namespace, &parsed.correlation_id);
    }

    let result = match serde_json::from_str::<KeyringRequest>(&parsed.payload_json) {
        Ok(req) => execute(req),
        Err(_) => CapabilityResult::Error(ERR_MALFORMED),
    };
    envelope_json(&parsed.namespace, &parsed.correlation_id, &result)
}

fn error_envelope(namespace: &str, correlation_id: &str) -> Zeroizing<String> {
    envelope_json(
        namespace,
        correlation_id,
        &CapabilityResult::Error(ERR_MALFORMED),
    )
}

fn execute(req: KeyringRequest) -> CapabilityResult {
    let store = SystemKeyringStore;
    execute_in(req, &store, legacy_sessions_dir().as_deref())
}

fn execute_in(
    req: KeyringRequest,
    store: &impl SessionSecretStore,
    legacy_base: Option<&Path>,
) -> CapabilityResult {
    match req {
        KeyringRequest::Store { account_id, secret } => {
            if !valid_account_id(&account_id) {
                return CapabilityResult::Error(ERR_PLATFORM);
            }
            let secret = Zeroizing::new(secret);
            match store.store_secret(&account_id, secret.as_str()) {
                Ok(()) => match remove_legacy_secret(legacy_base, &account_id) {
                    Ok(()) => CapabilityResult::Ok(None),
                    Err(code) => CapabilityResult::Error(code),
                },
                Err(StoreFailure::NotFound) => CapabilityResult::Error(ERR_PLATFORM),
                Err(StoreFailure::Error(code)) => CapabilityResult::Error(code),
            }
        }
        KeyringRequest::Retrieve { account_id } => {
            if !valid_account_id(&account_id) {
                return CapabilityResult::Error(ERR_PLATFORM);
            }
            match store.retrieve_secret(&account_id) {
                Ok(Some(secret)) => CapabilityResult::Ok(Some(secret)),
                Ok(None) => migrate_legacy_secret(store, legacy_base, &account_id),
                Err(StoreFailure::NotFound) => {
                    migrate_legacy_secret(store, legacy_base, &account_id)
                }
                Err(StoreFailure::Error(code)) => CapabilityResult::Error(code),
            }
        }
        KeyringRequest::Delete { account_id } => {
            if !valid_account_id(&account_id) {
                return CapabilityResult::Error(ERR_PLATFORM);
            }
            let keyring_result = store.delete_secret(&account_id);
            let legacy_result = remove_legacy_secret(legacy_base, &account_id);
            match (keyring_result, legacy_result) {
                (Ok(()), Ok(())) | (Err(StoreFailure::NotFound), Ok(())) => {
                    CapabilityResult::Ok(None)
                }
                (Err(StoreFailure::Error(code)), _) | (_, Err(code)) => {
                    CapabilityResult::Error(code)
                }
            }
        }
    }
}

fn valid_account_id(account_id: &str) -> bool {
    !account_id.is_empty()
        && !account_id.contains('/')
        && !account_id.contains('\\')
        && !account_id.contains('\0')
}

trait SessionSecretStore {
    fn store_secret(&self, account_id: &str, secret: &str) -> Result<(), StoreFailure>;
    fn retrieve_secret(&self, account_id: &str) -> Result<Option<Zeroizing<String>>, StoreFailure>;
    fn delete_secret(&self, account_id: &str) -> Result<(), StoreFailure>;
}

struct SystemKeyringStore;

impl SystemKeyringStore {
    fn entry(account_id: &str) -> Result<keyring::Entry, StoreFailure> {
        keyring::Entry::new(KEYRING_SERVICE, account_id).map_err(map_keyring_error)
    }
}

impl SessionSecretStore for SystemKeyringStore {
    fn store_secret(&self, account_id: &str, secret: &str) -> Result<(), StoreFailure> {
        Self::entry(account_id)?
            .set_password(secret)
            .map_err(map_keyring_error)
    }

    fn retrieve_secret(&self, account_id: &str) -> Result<Option<Zeroizing<String>>, StoreFailure> {
        match Self::entry(account_id)?.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_error(e)),
        }
    }

    fn delete_secret(&self, account_id: &str) -> Result<(), StoreFailure> {
        match Self::entry(account_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_error(e)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoreFailure {
    NotFound,
    Error(i32),
}

fn map_keyring_error(error: keyring::Error) -> StoreFailure {
    match error {
        keyring::Error::NoEntry => StoreFailure::NotFound,
        keyring::Error::BadEncoding(_) => StoreFailure::Error(ERR_MALFORMED),
        _ => StoreFailure::Error(ERR_PLATFORM),
    }
}

fn migrate_legacy_secret(
    store: &impl SessionSecretStore,
    legacy_base: Option<&Path>,
    account_id: &str,
) -> CapabilityResult {
    let Some(base) = legacy_base else {
        return CapabilityResult::NotFound;
    };
    let Some(path) = legacy_session_path_in(base, account_id) else {
        return CapabilityResult::Error(ERR_PLATFORM);
    };
    let secret = match fs::read_to_string(&path) {
        Ok(raw) => Zeroizing::new(raw),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return CapabilityResult::NotFound,
        Err(_) => return CapabilityResult::Error(ERR_PLATFORM),
    };
    match store.store_secret(account_id, secret.as_str()) {
        Ok(()) => match fs::remove_file(&path) {
            Ok(()) => CapabilityResult::Ok(Some(secret)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                CapabilityResult::Ok(Some(secret))
            }
            Err(_) => CapabilityResult::Error(ERR_PLATFORM),
        },
        Err(StoreFailure::NotFound) => CapabilityResult::Error(ERR_PLATFORM),
        Err(StoreFailure::Error(code)) => CapabilityResult::Error(code),
    }
}

fn remove_legacy_secret(legacy_base: Option<&Path>, account_id: &str) -> Result<(), i32> {
    let Some(base) = legacy_base else {
        return Ok(());
    };
    let Some(path) = legacy_session_path_in(base, account_id) else {
        return Err(ERR_PLATFORM);
    };
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ERR_PLATFORM),
    }
}

enum CapabilityResult {
    Ok(Option<Zeroizing<String>>),
    NotFound,
    Error(i32),
}

impl CapabilityResult {
    fn result_json(&self) -> Zeroizing<String> {
        let payload = match self {
            Self::Ok(secret) => ResultPayload {
                status: KeyringStatus::Ok,
                secret: secret.as_ref().map(|s| s.as_str()),
                os_status: None,
            },
            Self::NotFound => ResultPayload {
                status: KeyringStatus::NotFound,
                secret: None,
                os_status: None,
            },
            Self::Error(code) => ResultPayload {
                status: KeyringStatus::Error,
                secret: None,
                os_status: Some(*code),
            },
        };
        Zeroizing::new(serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string()))
    }
}

#[derive(Serialize)]
struct ResultPayload<'a> {
    status: KeyringStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    os_status: Option<i32>,
}

#[derive(Serialize)]
struct EnvelopePayload<'a> {
    namespace: &'a str,
    correlation_id: &'a str,
    result_json: &'a str,
}

fn envelope_json(
    namespace: &str,
    correlation_id: &str,
    result: &CapabilityResult,
) -> Zeroizing<String> {
    let result_json = result.result_json();
    let envelope = EnvelopePayload {
        namespace,
        correlation_id,
        result_json: result_json.as_str(),
    };
    Zeroizing::new(serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string()))
}

#[cfg(test)]
#[path = "keyring_tests.rs"]
mod tests;
