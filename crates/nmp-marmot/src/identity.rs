//! Chirp C-ABI glue for Rust-owned identity/Marmot orchestration.
//!
//! TODO: rename `nmp_app_chirp_identity_*` symbols to `nmp_marmot_identity_*`
//! now that the module lives in `nmp-marmot`. Out of scope for the relocation
//! PR; tracked by Opus direction reviews #50 and #68 as a separate naming
//! cleanup ("`nmp_app_chirp_*` symbols in Chirp-agnostic crates").

use std::ffi::{c_char, CStr};

use nmp_ffi::NmpApp;
use nostr::Keys;

use crate::ffi::{register_with_keys, MarmotHandle};

const CHIRP_IDENTITY_ACCOUNT_ID: &str = "chirp.marmot.cached_secret";

fn sign_in_and_register_marmot(
    app: *mut NmpApp,
    secret: &str,
    db_dir: Option<&str>,
) -> *mut MarmotHandle {
    let (Some(db_dir), Ok(keys)) = (db_dir, Keys::parse(secret)) else {
        return std::ptr::null_mut();
    };
    let db_path = format!("{}/marmot-mls-state.sqlite", db_dir.trim_end_matches('/'));
    register_with_keys(app, keys, &db_path)
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_identity_restore(
    app: *mut NmpApp,
    db_dir: *const c_char,
    test_nsec: *const c_char,
) -> *mut MarmotHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    let dir = c_str_opt(db_dir);
    let app_ref = unsafe { &*app };
    let secret =
        app_ref.restore_local_nsec_from_keyring(CHIRP_IDENTITY_ACCOUNT_ID, c_str_opt(test_nsec));
    let Some(secret) = secret else {
        return std::ptr::null_mut();
    };
    sign_in_and_register_marmot(app, &secret, dir.as_deref())
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_identity_sign_in_nsec(
    app: *mut NmpApp,
    secret: *const c_char,
    db_dir: *const c_char,
) -> *mut MarmotHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    let Some(secret) = c_str_opt(secret) else {
        return std::ptr::null_mut();
    };
    let dir = c_str_opt(db_dir);
    let app_ref = unsafe { &*app };
    let secret = app_ref.sign_in_local_nsec_with_keyring(CHIRP_IDENTITY_ACCOUNT_ID, secret);
    sign_in_and_register_marmot(app, &secret, dir.as_deref())
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_identity_remove_account(
    app: *mut NmpApp,
    identity_id: *const c_char,
) {
    if app.is_null() {
        return;
    }
    let Some(identity_id) = c_str_opt(identity_id) else {
        return;
    };
    let app_ref = unsafe { &*app };
    app_ref.remove_account_forgetting_keyring(CHIRP_IDENTITY_ACCOUNT_ID, identity_id);
}

fn c_str_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(|s| s.to_owned())
}
