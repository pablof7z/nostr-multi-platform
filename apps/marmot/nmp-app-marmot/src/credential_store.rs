//! Platform credential-store setup for Marmot SQLite encryption.

use keyring_core::set_default_store;

#[cfg(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
use apple_native_keyring_store::protected::Store as AppleStore;

#[must_use]
pub(crate) fn initialize() -> Option<bool> {
    #[cfg(any(
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
    let store = keyring_core::mock::Store::new().ok()?;
    set_default_store(store);
    Some(true)
}
