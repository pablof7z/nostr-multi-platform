//! C ABI constructor wrapper over `nmp-native-runtime`.

use crate::NmpApp;

/// Allocate a new native runtime app and return its opaque C handle.
#[no_mangle]
pub extern "C" fn nmp_app_new() -> *mut NmpApp {
    Box::into_raw(Box::new(nmp_native_runtime::new_app()))
}
