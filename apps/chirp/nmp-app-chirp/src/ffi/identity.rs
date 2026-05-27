//! Chirp identity wrappers around Marmot registration helpers.
//!
//! These symbols preserve Chirp's native ABI while keeping the reusable
//! `nmp-marmot` crate free of Chirp-specific symbol names.

use std::ffi::c_char;

use nmp_ffi::{nmp_app_remove_account, nmp_app_signin_nsec, NmpApp};
use nmp_marmot::ffi::{nmp_marmot_register, nmp_marmot_register_active, MarmotHandle};

use super::helpers::c_string_opt;

/// Rust-owned Chirp identity bootstrap: restore a persisted local secret
/// through the actor-owned session store and register Marmot. `test_nsec`
/// may be NULL; when non-NULL it signs in that injected secret and registers
/// Marmot for UI tests. Returns the Marmot handle or NULL.
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
    if !test_nsec.is_null() {
        nmp_app_signin_nsec(app, test_nsec);
        return nmp_marmot_register(app, test_nsec, db_dir);
    }
    nmp_marmot_register_active(app, db_dir)
}

/// Rust-owned nsec sign-in: sign in through the actor-owned identity reducer
/// and register Marmot from the same secret. Returns the Marmot handle or NULL.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_identity_sign_in_nsec(
    app: *mut NmpApp,
    secret: *const c_char,
    db_dir: *const c_char,
) -> *mut MarmotHandle {
    if app.is_null() || secret.is_null() {
        return std::ptr::null_mut();
    }
    nmp_app_signin_nsec(app, secret);
    nmp_marmot_register(app, secret, db_dir)
}

/// Rust-owned removal policy: remove the identity through the kernel actor.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_identity_remove_account(
    app: *mut NmpApp,
    identity_id: *const c_char,
) {
    if app.is_null() || c_string_opt(identity_id).is_none() {
        return;
    }
    nmp_app_remove_account(app, identity_id);
}
