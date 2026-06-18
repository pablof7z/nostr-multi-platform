//! Chirp-owned account-creation FFI wrapper.
//!
//! The generic `nmp_app_create_new_account` (in `nmp-ffi`) auto-follows nobody:
//! which accounts a fresh account follows is operator/product policy, and NMP no
//! longer hardcodes any default follow set (#1493). Chirp owns that policy in
//! `nmp_chirp_config::chirp_default_follows`, and this wrapper threads it into
//! the create-account command via the shared
//! `nmp_ffi::create_new_account_with_initial_follows` helper.
//!
//! This is the exact Rust-owned pattern the relay bootstrap already uses
//! (`nmp_app_chirp_seed_default_relays` wraps `chirp_default_relay_bootstrap`):
//! the seed follows never transit the thin Swift/Kotlin shell — the shell calls
//! this Chirp symbol with the SAME `(profile_json, relays_json, mls,
//! make_active)` arguments it passed to the generic symbol, and the follows are
//! injected here in Rust.
//!
//! D6: fire-and-forget. A null app or undecodable JSON degrades to a `false`
//! return (the underlying helper surfaces a toast) rather than raising across
//! the FFI.

use std::ffi::c_char;

use nmp_ffi::NmpApp;

/// Create a new Chirp account, auto-following Chirp's product seed set
/// (`nmp_chirp_config::chirp_default_follows`).
///
/// Drop-in replacement for `nmp_app_create_new_account` on the Chirp path —
/// same arguments, but the fresh account is seeded with Chirp's default follows
/// (kind:3 contacts prepopulate + cold-start publish) instead of an empty set.
///
/// Returns `true` when the create-account command was dispatched, `false` on a
/// null app or undecodable `profile_json` / `relays_json`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_create_new_account(
    app: *mut NmpApp,
    profile_json: *const c_char,
    relays_json: *const c_char,
    mls: bool,
    make_active: u8,
) -> bool {
    let follows = nmp_chirp_config::chirp_default_follows()
        .iter()
        .map(|p| (*p).to_string())
        .collect::<Vec<String>>();
    nmp_ffi::create_new_account_with_initial_follows(
        app,
        profile_json,
        relays_json,
        mls,
        make_active,
        follows,
    )
}
