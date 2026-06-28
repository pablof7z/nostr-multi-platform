//! Persistent storage-path configuration for native app shells.

use crate::{NmpApp, NmpConfigStatus};

impl NmpApp {
    /// The configured LMDB storage path, if one was set before actor start.
    #[must_use]
    pub fn storage_path_for_start(&self) -> Option<String> {
        self.composition
            .storage_path
            .lock()
            .ok()
            .and_then(|g| g.clone())
    }
    /// Set the persistent storage directory for the LMDB `EventStore` backend.
    pub fn set_storage_path(&self, path: Option<String>) -> NmpConfigStatus {
        if let Err(status) =
            self.ensure_prestart_config("storage_path", "storage_path", "set_storage_path")
        {
            return status;
        }
        let resolved = path.and_then(|path| {
            let trimmed = path.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });
        let Ok(mut slot) = self.composition.storage_path.lock() else {
            return NmpConfigStatus::Unavailable;
        };
        self.record_slot_decision("storage_path", "storage_path", slot.is_some());
        *slot = resolved;
        NmpConfigStatus::Ok
    }
}
