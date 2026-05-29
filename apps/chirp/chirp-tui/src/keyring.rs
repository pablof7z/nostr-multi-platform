//! Host-side keyring capability handler for chirp-tui.
//!
//! Wires the NMP `KeyringCapability` socket to file-based session storage so
//! the local nsec persists across restarts without triggering an OS secret
//! store dialog (e.g. the macOS Keychain access prompt). Each account's secret
//! is stored as a plain file under the platform data dir.

use std::ffi::{c_char, c_void, CStr, CString};
use std::fs;
use std::path::{Path, PathBuf};

use nmp_core::substrate::{
    CapabilityEnvelope, CapabilityModule, KeyringCapability, KeyringRequest, KeyringResult,
};

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

/// Directory holding one secret file per account.
fn sessions_dir() -> Option<PathBuf> {
    chirp_data_dir().map(|d| d.join("sessions"))
}

/// Path to the secret file for `account_id` under `base`.
///
/// `account_id` is a nostr pubkey (hex), which is already path-safe. We still
/// reject any value containing a path separator or NUL to avoid traversal.
fn session_path_in(base: &Path, account_id: &str) -> Option<PathBuf> {
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
    let Some(base) = sessions_dir() else {
        return KeyringResult::error(-1);
    };
    execute_in(req, &base)
}

/// Backend for [`execute`] parameterized on the sessions directory so it can be
/// exercised against an isolated temp dir in tests without touching the real
/// store or mutating global environment variables.
fn execute_in(req: KeyringRequest, base: &Path) -> KeyringResult {
    match req {
        KeyringRequest::Store { account_id, secret } => {
            let Some(path) = session_path_in(base, &account_id) else {
                return KeyringResult::error(-1);
            };
            if fs::create_dir_all(base).is_err() {
                return KeyringResult::error(-1);
            }
            if fs::write(&path, secret.as_bytes()).is_err() {
                return KeyringResult::error(-1);
            }
            // Restrict the on-disk secret to the owner (best effort).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            }
            KeyringResult::ok(None)
        }
        KeyringRequest::Retrieve { account_id } => {
            let Some(path) = session_path_in(base, &account_id) else {
                return KeyringResult::error(-1);
            };
            match fs::read_to_string(&path) {
                Ok(secret) => KeyringResult::ok(Some(secret)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => KeyringResult::not_found(),
                Err(_) => KeyringResult::error(-1),
            }
        }
        KeyringRequest::Delete { account_id } => {
            let Some(path) = session_path_in(base, &account_id) else {
                return KeyringResult::error(-1);
            };
            match fs::remove_file(&path) {
                Ok(()) => KeyringResult::ok(None),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => KeyringResult::ok(None),
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

    #[test]
    fn session_path_rejects_traversal() {
        let base = Path::new("/tmp/chirp-test-base");
        assert!(session_path_in(base, "").is_none());
        assert!(session_path_in(base, "../escape").is_none());
        assert!(session_path_in(base, "a/b").is_none());
        assert!(session_path_in(base, "deadbeef").is_some());
    }

    #[test]
    fn store_retrieve_delete_roundtrip() {
        // Isolate to a unique temp dir; no global env mutation, so this is safe
        // to run in parallel with the other tests in this binary.
        let base = std::env::temp_dir().join(format!(
            "chirp-tui-keyring-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&base);

        let account = "deadbeefcafe";
        let secret = "nsec1examplesecretvalue";

        // Missing → not_found.
        assert_eq!(
            execute_in(KeyringRequest::Retrieve { account_id: account.to_string() }, &base),
            KeyringResult::not_found()
        );

        // Store → ok.
        assert_eq!(
            execute_in(
                KeyringRequest::Store {
                    account_id: account.to_string(),
                    secret: secret.to_string(),
                },
                &base
            ),
            KeyringResult::ok(None)
        );

        // Retrieve → ok(secret).
        assert_eq!(
            execute_in(KeyringRequest::Retrieve { account_id: account.to_string() }, &base),
            KeyringResult::ok(Some(secret.to_string()))
        );

        // Delete → ok.
        assert_eq!(
            execute_in(KeyringRequest::Delete { account_id: account.to_string() }, &base),
            KeyringResult::ok(None)
        );

        // Deleting again (missing) → still ok.
        assert_eq!(
            execute_in(KeyringRequest::Delete { account_id: account.to_string() }, &base),
            KeyringResult::ok(None)
        );

        let _ = fs::remove_dir_all(&base);
    }
}
