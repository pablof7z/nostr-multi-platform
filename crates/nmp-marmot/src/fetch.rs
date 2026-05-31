//! Key-package fetch C-ABI helper.

use std::ffi::c_char;

use crate::ffi::{c_str_opt, MarmotHandle};

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_marmot_fetch_key_packages(
    handle: *mut MarmotHandle,
    pubkeys_json: *const c_char,
) {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return;
    };
    if handle.app.is_null() {
        return;
    }
    let Some(json_str) = c_str_opt(pubkeys_json) else {
        return;
    };
    let Ok(pubkeys) = serde_json::from_str::<Vec<String>>(&json_str) else {
        return;
    };
    let app_ref = unsafe { &*handle.app };
    for pk_str in pubkeys {
        let Ok(pk) = nostr::PublicKey::parse(&pk_str) else {
            continue;
        };
        app_ref.push_interest(crate::interest::key_package_lookup_interest(&pk.to_hex()));
    }
}
