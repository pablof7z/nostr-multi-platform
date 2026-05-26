//! Chirp identity wrappers around Marmot's app-neutral keyring helpers.
//!
//! These symbols preserve Chirp's native ABI while keeping the reusable
//! `nmp-marmot` crate free of Chirp-specific symbol names and keyring policy.

use std::ffi::c_char;

use nmp_ffi::NmpApp;
use nmp_marmot::ffi::MarmotHandle;

use super::helpers::c_string_opt;

const CHIRP_MARMOT_KEYRING_ACCOUNT_ID: &str = "chirp.marmot.cached_secret";

/// Rust-owned Chirp identity bootstrap: restore a persisted local secret
/// through the native keyring capability, sign in through the kernel actor,
/// and register Marmot. `test_nsec` may be NULL; when non-NULL it overrides
/// keyring recall for UI tests. Returns the Marmot handle or NULL.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_identity_restore(
    app: *mut NmpApp,
    db_dir: *const c_char,
    test_nsec: *const c_char,
) -> *mut MarmotHandle {
    let dir = c_string_opt(db_dir);
    nmp_marmot::identity::restore_identity_with_keyring_account(
        app,
        CHIRP_MARMOT_KEYRING_ACCOUNT_ID,
        dir.as_deref(),
        c_string_opt(test_nsec),
    )
}

/// Rust-owned nsec sign-in: persist through keyring capability, sign in, and
/// register Marmot. Returns the Marmot handle or NULL.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_identity_sign_in_nsec(
    app: *mut NmpApp,
    secret: *const c_char,
    db_dir: *const c_char,
) -> *mut MarmotHandle {
    let Some(secret) = c_string_opt(secret) else {
        return std::ptr::null_mut();
    };
    let dir = c_string_opt(db_dir);
    nmp_marmot::identity::sign_in_nsec_with_keyring_account(
        app,
        CHIRP_MARMOT_KEYRING_ACCOUNT_ID,
        secret,
        dir.as_deref(),
    )
}

/// Rust-owned removal policy: forget Chirp's persisted local secret and remove
/// the identity through the kernel actor.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_identity_remove_account(
    app: *mut NmpApp,
    identity_id: *const c_char,
) {
    let Some(identity_id) = c_string_opt(identity_id) else {
        return;
    };
    nmp_marmot::identity::remove_identity_with_keyring_account(
        app,
        CHIRP_MARMOT_KEYRING_ACCOUNT_ID,
        identity_id,
    );
}
