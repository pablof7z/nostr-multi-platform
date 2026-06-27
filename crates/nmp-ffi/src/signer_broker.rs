//! NIP-46 signer-broker C ABI wrappers over native-runtime.

use std::ffi::{c_char, CStr, CString};

use crate::{app_ref, NmpApp, NmpConfigStatus};

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_signer_broker_init(app: *mut NmpApp) -> u32 {
    let Some(app) = app_ref(app) else {
        return NmpConfigStatus::NullApp.code();
    };
    app.init_signer_broker().code()
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_cancel_bunker_handshake(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.cancel_bunker_handshake();
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_nostrconnect_uri(
    app: *mut NmpApp,
    callback_scheme: *const c_char,
) -> *mut c_char {
    let Some(app) = app_ref(app) else {
        return std::ptr::null_mut();
    };
    let callback = if callback_scheme.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null means a valid C string.
        unsafe { CStr::from_ptr(callback_scheme).to_str() }
            .ok()
            .filter(|s| !s.is_empty())
    };
    match app.nostrconnect_uri(callback) {
        Some(uri) => CString::new(uri)
            .unwrap_or_else(|_| c"".to_owned())
            .into_raw(),
        None => std::ptr::null_mut(),
    }
}
