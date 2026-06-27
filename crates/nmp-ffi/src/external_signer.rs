//! NIP-55 external-signer C ABI wrappers.

use std::ffi::{c_char, CStr};

use crate::{app_ref, c_string_argument, NmpApp};

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_external_signer_init(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.init_external_signer();
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_signin_nip55(app: *mut NmpApp, signer_package: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let package = if signer_package.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null means a valid C string for the
        // call duration. Invalid UTF-8 degrades to no package hint.
        unsafe { CStr::from_ptr(signer_package).to_str() }
            .ok()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    app.signin_nip55(package);
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_deliver_external_signer_response(
    app: *mut NmpApp,
    response_json: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(response) = c_string_argument(response_json) else {
        return;
    };
    app.deliver_external_signer_response(&response);
}
