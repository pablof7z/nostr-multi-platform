//! Persistent storage-path configuration for native app shells.

use std::ffi::c_char;

use crate::{NmpApp, app_ref, c_optional_string_argument};

impl NmpApp {
    /// The configured LMDB storage path, if one was set before actor start.
    #[must_use]
    pub fn storage_path_for_start(&self) -> Option<String> {
        self.storage_path.lock().ok().and_then(|g| g.clone())
    }
}

/// Set the persistent storage directory for the LMDB `EventStore` backend.
///
/// Call before `nmp_app_start`. Null, empty, whitespace, or invalid UTF-8
/// clears the path, making the kernel fall back to its configured default.
#[no_mangle]
pub extern "C" fn nmp_app_set_storage_path(app: *mut NmpApp, path: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let resolved = c_optional_string_argument(path);
    let Ok(mut slot) = app.storage_path.lock() else {
        return;
    };
    *slot = resolved;
}
