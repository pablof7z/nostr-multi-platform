//! Persistent storage-path configuration for native app shells.

use std::ffi::c_char;

use crate::{app_ref, c_optional_string_argument, NmpApp, NmpConfigStatus};

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
}

/// Set the persistent storage directory for the LMDB `EventStore` backend.
///
/// Call before `nmp_app_start`. Null, empty, whitespace, or invalid UTF-8
/// clears the path, making the kernel fall back to its configured default.
///
/// Returns [`NmpConfigStatus::AlreadyStarted`] when called after start; the
/// existing start-time path is left untouched and the composition ledger records
/// `DroppedLateWiring`.
#[no_mangle]
pub extern "C" fn nmp_app_set_storage_path(app: *mut NmpApp, path: *const c_char) -> u32 {
    let Some(app) = app_ref(app) else {
        return NmpConfigStatus::NullApp.code();
    };
    if let Err(status) =
        app.ensure_prestart_config("storage_path", "storage_path", "nmp_app_set_storage_path")
    {
        return status.code();
    }
    let resolved = c_optional_string_argument(path);
    let Ok(mut slot) = app.composition.storage_path.lock() else {
        return NmpConfigStatus::Unavailable.code();
    };
    app.record_slot_decision("storage_path", "storage_path", slot.is_some());
    *slot = resolved;
    NmpConfigStatus::Ok.code()
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
        assert_eq!(
            nmp_app_set_storage_path(app, c_path.as_ptr()),
            NmpConfigStatus::Ok.code()
        );

        let app_ref = unsafe { &*app };
        assert_eq!(app_ref.storage_path_for_start(), Some(path_str));

        nmp_app_start(app, 256, 4);
        assert_eq!(nmp_app_is_alive(app), 1, "start should spawn actor once");
        nmp_app_free(app);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn storage_path_after_start_is_rejected_and_recorded() {
        let app = nmp_app_new();
        let first_dir =
            std::env::temp_dir().join(format!("nmp-storage-first-{}", std::process::id()));
        let second_dir =
            std::env::temp_dir().join(format!("nmp-storage-second-{}", std::process::id()));
        std::fs::create_dir_all(&first_dir).expect("create first temp storage dir");
        std::fs::create_dir_all(&second_dir).expect("create second temp storage dir");
        let first = first_dir.to_string_lossy().to_string();
        let second = second_dir.to_string_lossy().to_string();
        let c_first = CString::new(first.clone()).expect("first path has no nul");
        let c_second = CString::new(second).expect("second path has no nul");

        assert_eq!(
            nmp_app_set_storage_path(app, c_first.as_ptr()),
            NmpConfigStatus::Ok.code()
        );
        nmp_app_start(app, 256, 4);
        assert_eq!(
            nmp_app_set_storage_path(app, c_second.as_ptr()),
            NmpConfigStatus::AlreadyStarted.code()
        );

        let app_ref = unsafe { &*app };
        assert_eq!(app_ref.storage_path_for_start(), Some(first));
        let records = app_ref.composition_ledger().to_json()["records"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            records.iter().any(|record| {
                record["seam"] == "storage_path" && record["disposition"] == "DroppedLateWiring"
            }),
            "late storage setter should be visible in the composition ledger"
        );

        nmp_app_free(app);
        let _ = std::fs::remove_dir_all(first_dir);
        let _ = std::fs::remove_dir_all(second_dir);
    }
}
