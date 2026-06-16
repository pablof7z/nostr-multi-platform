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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        nmp_app_free, nmp_app_is_alive, nmp_app_lifecycle_foreground, nmp_app_new, nmp_app_start,
    };
    use std::ffi::CString;

    #[test]
    fn storage_path_can_be_set_after_prestart_command_before_start() {
        let app = nmp_app_new();
        nmp_app_lifecycle_foreground(app);
        assert_eq!(
            nmp_app_is_alive(app),
            0,
            "pre-start command must not spawn actor"
        );

        let path = std::env::temp_dir().join(format!("nmp-passive-start-{}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp storage dir");
        let path_str = path.to_string_lossy().to_string();
        let c_path = CString::new(path_str.clone()).expect("temp path has no nul");
        nmp_app_set_storage_path(app, c_path.as_ptr());

        let app_ref = unsafe { &*app };
        assert_eq!(app_ref.storage_path_for_start(), Some(path_str));

        nmp_app_start(app, 0, 256, 4);
        assert_eq!(nmp_app_is_alive(app), 1, "start should spawn actor once");
        nmp_app_free(app);
        let _ = std::fs::remove_dir_all(path);
    }
}
