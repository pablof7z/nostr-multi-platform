//! T66a identity / publish / multi-account / relay-edit FFI wrappers.
//!
//! Split out of `ffi/mod.rs` to keep both files under the 500-LOC hard cap.
//! These reuse the parent module's validated-argument helpers
//! (`app_ref`, `c_string_argument`, `c_optional_string_argument`) and the
//! shared `NmpApp` handle; the symbols stay `#[no_mangle] extern "C"` so the
//! Swift bridge sees a flat C ABI regardless of the Rust module split.

use super::{app_ref, c_optional_string_argument, c_string_argument, NmpApp};
use crate::actor::ActorCommand;
use crate::kernel::{is_hex_id, is_hex_pubkey};
use std::ffi::c_char;

#[no_mangle]
pub extern "C" fn nmp_app_signin_nsec(app: *mut NmpApp, secret: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(secret) = c_string_argument(secret) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::SignInNsec { secret });
}

#[no_mangle]
pub extern "C" fn nmp_app_signin_bunker(app: *mut NmpApp, uri: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::SignInBunker { uri });
}

#[no_mangle]
pub extern "C" fn nmp_app_create_new_account(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::CreateAccount);
}

#[no_mangle]
pub extern "C" fn nmp_app_switch_active(app: *mut NmpApp, identity_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(identity_id) = c_string_argument(identity_id) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::SwitchActive { identity_id });
}

#[no_mangle]
pub extern "C" fn nmp_app_remove_account(app: *mut NmpApp, identity_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(identity_id) = c_string_argument(identity_id) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::RemoveAccount { identity_id });
}

#[no_mangle]
pub extern "C" fn nmp_app_publish_note(
    app: *mut NmpApp,
    content: *const c_char,
    reply_to_id_or_null: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(content) = c_string_argument(content) else {
        return;
    };
    let reply_to_id = c_optional_string_argument(reply_to_id_or_null);
    let _ = app.tx.send(ActorCommand::PublishNote {
        content,
        reply_to_id,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_react(
    app: *mut NmpApp,
    target_event_id: *const c_char,
    reaction: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(target_event_id) = c_string_argument(target_event_id) else {
        return;
    };
    if !is_hex_id(&target_event_id) {
        return;
    }
    let reaction = c_optional_string_argument(reaction).unwrap_or_else(|| "+".to_string());
    let _ = app.tx.send(ActorCommand::React {
        target_event_id,
        reaction,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_follow(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }
    let _ = app.tx.send(ActorCommand::Follow { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_unfollow(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }
    let _ = app.tx.send(ActorCommand::Unfollow { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_add_relay(
    app: *mut NmpApp,
    url: *const c_char,
    role: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(url) = c_string_argument(url) else {
        return;
    };
    let role = c_optional_string_argument(role).unwrap_or_else(|| "both".to_string());
    let _ = app.tx.send(ActorCommand::AddRelay { url, role });
}

#[no_mangle]
pub extern "C" fn nmp_app_remove_relay(app: *mut NmpApp, url: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(url) = c_string_argument(url) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::RemoveRelay { url });
}

#[no_mangle]
pub extern "C" fn nmp_app_open_timeline(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::OpenTimeline);
}

