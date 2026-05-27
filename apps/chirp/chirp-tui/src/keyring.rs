//! Host-side keyring capability handler for chirp-tui.
//!
//! Wires the NMP `KeyringCapability` socket to the `keyring` crate so the
//! local nsec persists across restarts via the OS secret store (macOS
//! Keychain, Linux Secret Service, Windows Credential Manager).

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;

use keyring::Entry;
use nmp_core::substrate::{
    CapabilityEnvelope, CapabilityModule, KeyringCapability, KeyringRequest, KeyringResult,
};

const KEYRING_SERVICE: &str = "chirp-tui";

pub(crate) fn chirp_data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/chirp-tui"))
    } else if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        Some(PathBuf::from(xdg).join("chirp-tui"))
    } else {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(".local/share/chirp-tui"))
    }
}

pub(crate) extern "C" fn keyring_handler(
    _ctx: *mut c_void,
    request_json: *const c_char,
) -> *mut c_char {
    let envelope_json = build_envelope_json(request_json);
    CString::new(envelope_json)
        .unwrap_or_else(|_| CString::new("{}").expect("static literal has no NUL"))
        .into_raw()
}

fn build_envelope_json(request_json: *const c_char) -> String {
    if request_json.is_null() {
        return error_envelope("", "");
    }
    let request_str = match unsafe { CStr::from_ptr(request_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return error_envelope("", ""),
    };
    let parsed: serde_json::Value = serde_json::from_str(request_str).unwrap_or_default();
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
        Ok(req) => execute(req),
        Err(_) => KeyringResult::error(-50),
    };

    let envelope = CapabilityEnvelope {
        namespace: KeyringCapability::NAMESPACE.to_string(),
        correlation_id,
        result_json: serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string()),
    };
    serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string())
}

fn error_envelope(namespace: &str, correlation_id: &str) -> String {
    let envelope = CapabilityEnvelope {
        namespace: namespace.to_string(),
        correlation_id: correlation_id.to_string(),
        result_json: serde_json::to_string(&KeyringResult::error(-50))
            .unwrap_or_else(|_| "{}".to_string()),
    };
    serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string())
}

fn execute(req: KeyringRequest) -> KeyringResult {
    match req {
        KeyringRequest::Store { account_id, secret } => {
            match Entry::new(KEYRING_SERVICE, &account_id) {
                Ok(entry) => match entry.set_password(&secret) {
                    Ok(()) => KeyringResult::ok(None),
                    Err(_) => KeyringResult::error(-1),
                },
                Err(_) => KeyringResult::error(-1),
            }
        }
        KeyringRequest::Retrieve { account_id } => {
            match Entry::new(KEYRING_SERVICE, &account_id) {
                Ok(entry) => match entry.get_password() {
                    Ok(secret) => KeyringResult::ok(Some(secret)),
                    Err(keyring::Error::NoEntry) => KeyringResult::not_found(),
                    Err(_) => KeyringResult::error(-1),
                },
                Err(_) => KeyringResult::error(-1),
            }
        }
        KeyringRequest::Delete { account_id } => {
            match Entry::new(KEYRING_SERVICE, &account_id) {
                Ok(entry) => match entry.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => KeyringResult::ok(None),
                    Err(_) => KeyringResult::error(-1),
                },
                Err(_) => KeyringResult::error(-1),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_returns_some_path() {
        if std::env::var_os("HOME").is_some() || std::env::var_os("XDG_DATA_HOME").is_some() {
            assert!(chirp_data_dir().is_some());
        }
    }

    #[test]
    fn null_request_returns_error_envelope() {
        let out = build_envelope_json(std::ptr::null());
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let result: KeyringResult =
            serde_json::from_str(v["result_json"].as_str().unwrap()).unwrap();
        assert_eq!(result, KeyringResult::error(-50));
    }
}
