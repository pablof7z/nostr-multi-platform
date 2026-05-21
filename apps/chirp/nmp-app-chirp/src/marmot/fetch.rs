//! Key-package fetch C-ABI helper.

use std::ffi::c_char;

use nmp_core::{ActorCommand, KernelAction};

use super::ffi::{c_str_opt, MarmotHandle};

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_marmot_fetch_key_packages(
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
    let sender = app_ref.actor_sender();
    for pk_str in pubkeys {
        let Ok(pk) = nostr::PublicKey::parse(&pk_str) else {
            continue;
        };
        let _ = sender.send(ActorCommand::Kernel(KernelAction::OpenView {
            namespace: nmp_marmot::view::KeyPackageLookupView::NAMESPACE.to_string(),
            key: pk.to_hex(),
        }));
    }
}
