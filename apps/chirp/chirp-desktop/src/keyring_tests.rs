use super::*;
use nmp_core::substrate::{CapabilityEnvelope, KeyringResult};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
struct MemoryStore {
    slots: Mutex<HashMap<String, String>>,
    fail_store: bool,
}

impl MemoryStore {
    fn get(&self, account_id: &str) -> Option<String> {
        self.slots.lock().unwrap().get(account_id).cloned()
    }
}

impl SessionSecretStore for MemoryStore {
    fn store_secret(&self, account_id: &str, secret: &str) -> Result<(), StoreFailure> {
        if self.fail_store {
            return Err(StoreFailure::Error(ERR_PLATFORM));
        }
        self.slots
            .lock()
            .unwrap()
            .insert(account_id.to_string(), secret.to_string());
        Ok(())
    }

    fn retrieve_secret(&self, account_id: &str) -> Result<Option<Zeroizing<String>>, StoreFailure> {
        Ok(self
            .slots
            .lock()
            .unwrap()
            .get(account_id)
            .cloned()
            .map(Zeroizing::new))
    }

    fn delete_secret(&self, account_id: &str) -> Result<(), StoreFailure> {
        self.slots.lock().unwrap().remove(account_id);
        Ok(())
    }
}

fn decode_result(result: &CapabilityResult) -> KeyringResult {
    serde_json::from_str(result.result_json().as_str()).unwrap()
}

fn execute_result(
    req: KeyringRequest,
    store: &impl SessionSecretStore,
    legacy_base: Option<&Path>,
) -> KeyringResult {
    decode_result(&execute_in(req, store, legacy_base))
}

fn isolated_base(slug: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "chirp-desktop-keyring-{slug}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

#[test]
fn data_dir_returns_some_path() {
    if std::env::var_os("HOME").is_some() || std::env::var_os("XDG_DATA_HOME").is_some() {
        assert!(chirp_data_dir().is_some());
    }
}

#[test]
fn null_request_returns_error_envelope() {
    let out = build_envelope_json(std::ptr::null());
    let envelope: CapabilityEnvelope = serde_json::from_str(out.as_str()).unwrap();
    let result: KeyringResult = serde_json::from_str(&envelope.result_json).unwrap();
    assert_eq!(result, KeyringResult::error(ERR_MALFORMED));
}

#[test]
fn session_path_rejects_traversal() {
    let base = Path::new("/tmp/chirp-test-base");
    assert!(legacy_session_path_in(base, "").is_none());
    assert!(legacy_session_path_in(base, "../escape").is_none());
    assert!(legacy_session_path_in(base, "a/b").is_none());
    assert!(legacy_session_path_in(base, "deadbeef").is_some());
}

#[test]
fn store_retrieve_delete_roundtrip() {
    let base = isolated_base("roundtrip");
    let _ = fs::remove_dir_all(&base);

    let account = "deadbeefcafe";
    let secret = "nsec1examplesecretvalue";
    let store = MemoryStore::default();

    assert_eq!(
        execute_result(
            KeyringRequest::Retrieve {
                account_id: account.to_string()
            },
            &store,
            Some(&base)
        ),
        KeyringResult::not_found()
    );

    assert_eq!(
        execute_result(
            KeyringRequest::Store {
                account_id: account.to_string(),
                secret: secret.to_string(),
            },
            &store,
            Some(&base)
        ),
        KeyringResult::ok(None)
    );
    assert_eq!(store.get(account).as_deref(), Some(secret));
    assert!(!legacy_session_path_in(&base, account).unwrap().exists());

    assert_eq!(
        execute_result(
            KeyringRequest::Retrieve {
                account_id: account.to_string()
            },
            &store,
            Some(&base)
        ),
        KeyringResult::ok(Some(secret.to_string()))
    );

    assert_eq!(
        execute_result(
            KeyringRequest::Delete {
                account_id: account.to_string()
            },
            &store,
            Some(&base)
        ),
        KeyringResult::ok(None)
    );
    assert!(store.get(account).is_none());

    assert_eq!(
        execute_result(
            KeyringRequest::Delete {
                account_id: account.to_string()
            },
            &store,
            Some(&base)
        ),
        KeyringResult::ok(None)
    );

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn retrieve_migrates_legacy_plaintext_file_into_keyring() {
    let base = isolated_base("migration");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    let account = "feedface";
    let secret = "nsec1legacysecret";
    let legacy_path = legacy_session_path_in(&base, account).unwrap();
    fs::write(&legacy_path, secret).unwrap();

    let store = MemoryStore::default();
    assert_eq!(
        execute_result(
            KeyringRequest::Retrieve {
                account_id: account.to_string()
            },
            &store,
            Some(&base)
        ),
        KeyringResult::ok(Some(secret.to_string()))
    );
    assert_eq!(store.get(account).as_deref(), Some(secret));
    assert!(
        !legacy_path.exists(),
        "successful migration must remove the plaintext legacy file"
    );

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn failed_legacy_migration_does_not_return_plaintext_secret() {
    let base = isolated_base("migration-fail");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    let account = "badcafe";
    let secret = "nsec1legacysecret";
    let legacy_path = legacy_session_path_in(&base, account).unwrap();
    fs::write(&legacy_path, secret).unwrap();

    let store = MemoryStore {
        slots: Mutex::new(HashMap::new()),
        fail_store: true,
    };
    assert_eq!(
        execute_result(
            KeyringRequest::Retrieve {
                account_id: account.to_string()
            },
            &store,
            Some(&base)
        ),
        KeyringResult::error(ERR_PLATFORM)
    );
    assert!(
        legacy_path.exists(),
        "failed migration leaves the old file for an explicit later retry"
    );

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn delete_removes_legacy_plaintext_file() {
    let base = isolated_base("delete");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();

    let account = "cafebabe";
    let legacy_path = legacy_session_path_in(&base, account).unwrap();
    fs::write(&legacy_path, "nsec1old").unwrap();

    let store = MemoryStore::default();
    assert_eq!(
        execute_result(
            KeyringRequest::Delete {
                account_id: account.to_string()
            },
            &store,
            Some(&base)
        ),
        KeyringResult::ok(None)
    );
    assert!(!legacy_path.exists());

    let _ = fs::remove_dir_all(&base);
}
