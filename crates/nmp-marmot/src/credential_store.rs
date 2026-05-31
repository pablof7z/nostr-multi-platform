//! Platform credential-store setup for Marmot SQLite encryption.
//!
//! # Headless / CI escape hatch
//!
//! Setting the environment variable `NMP_MARMOT_MOCK_KEYRING=1` (or `true`,
//! case-insensitive) before the process starts causes `initialize()` to
//! install the in-memory mock store **on every platform**, bypassing the
//! Apple Keychain or any other native credential store.
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

use keyring_core::set_default_store;
use std::sync::{Arc, OnceLock};

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
use apple_native_keyring_store::protected::Store as AppleStore;

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

#[must_use]
pub(crate) fn initialize() -> Option<bool> {
    // Escape hatch: if the caller has opted in via env var, install the
    // in-memory mock store unconditionally — before any platform check.
    // Off by default; production builds never set this variable.
    if env_requests_mock() {
        return install_mock_store();
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    ))]
    {
        if let Ok(store) = AppleStore::new() {
            set_default_store(store);
            return Some(false);
        }
        return install_mock_store();
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    )))]
    {
        install_mock_store()
    }
}

#[must_use]
pub(crate) fn install_mock_store() -> Option<bool> {
    static MOCK_STORE: OnceLock<Arc<keyring_core::CredentialStore>> = OnceLock::new();
    let store = Arc::clone(MOCK_STORE.get_or_init(|| {
        let store: Arc<keyring_core::CredentialStore> =
            keyring_core::mock::Store::new().expect("mock keyring store");
        store
    }));
    set_default_store(store);
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize all tests that read or write NMP_MARMOT_MOCK_KEYRING so they
    // don't race each other (env vars are process-global).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const VAR: &str = "NMP_MARMOT_MOCK_KEYRING";

    // ── env_requests_mock() parsing — no globals touched ────────────────────

    #[test]
    fn env_mock_unset_is_false() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(VAR);
        assert!(!env_requests_mock());
    }

    #[test]
    fn env_mock_recognized_opt_in_values() {
        let _guard = ENV_LOCK.lock().unwrap();
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
        let _guard = ENV_LOCK.lock().unwrap();
        for val in ["0", "false", "no", "off", "", "maybe", "2", "truetrue"] {
            std::env::set_var(VAR, val);
            assert!(
                !env_requests_mock(),
                "expected no opt-in for NMP_MARMOT_MOCK_KEYRING={val:?}"
            );
        }
        std::env::remove_var(VAR);
    }

    // ── initialize() with escape hatch active — all platforms ───────────────

    #[test]
    fn initialize_returns_mock_when_env_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(VAR, "1");
        // initialize() must return Some(true) — the mock-chosen branch —
        // regardless of platform (Apple Keychain check is bypassed).
        let result = initialize();
        std::env::remove_var(VAR);
        assert_eq!(
            result,
            Some(true),
            "NMP_MARMOT_MOCK_KEYRING=1 must select the in-memory mock store on all platforms"
        );
    }
}
