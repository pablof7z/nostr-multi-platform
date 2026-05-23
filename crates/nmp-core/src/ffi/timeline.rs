//! Timeline / profile FFI wrappers — open/close author + thread + firehose,
//! `nostr:` URI routing, and profile claim/release.
//!
//! Split out of `ffi/mod.rs` to keep both files under the 300-LOC soft cap.
//! These reuse the parent module's validated-argument helpers (`app_ref`,
//! `c_string_argument`) and the shared `NmpApp` handle; the symbols stay
//! `#[no_mangle] extern "C"` so the Swift bridge sees a flat C ABI regardless
//! of the Rust module split.

use super::{app_ref, c_string_argument, NmpApp};
use crate::actor::ActorCommand;
use crate::kernel::{is_hex_id, is_hex_pubkey};
use std::ffi::c_char;

#[no_mangle]
pub extern "C" fn nmp_app_open_author(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    app.send_cmd(ActorCommand::OpenAuthor { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_open_thread(app: *mut NmpApp, event_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(event_id) = c_string_argument(event_id) else {
        return;
    };
    if !is_hex_id(&event_id) {
        return;
    }

    app.send_cmd(ActorCommand::OpenThread { event_id });
}

#[no_mangle]
pub extern "C" fn nmp_app_open_firehose_tag(app: *mut NmpApp, tag: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(tag) = c_string_argument(tag) else {
        return;
    };

    app.send_cmd(ActorCommand::OpenFirehoseTag { tag });
}

/// Open whatever a `nostr:` URI (or bare NIP-19 entity) points at (T95/T80).
/// Routed through the `KernelAction` reducer: success registers the resolved
/// interest + pushes `ViewOpened`, failure pushes `UriRejected`. FFI-clean
/// (D6): a null/invalid argument is a silent no-op, never a panic.
#[no_mangle]
pub extern "C" fn nmp_app_open_uri(app: *mut NmpApp, uri: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };

    app.send_cmd(ActorCommand::Kernel(crate::app::KernelAction::OpenUri {
        uri,
    }));
}

#[no_mangle]
pub extern "C" fn nmp_app_claim_profile(
    app: *mut NmpApp,
    pubkey: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    app.send_cmd(ActorCommand::ClaimProfile {
        pubkey,
        consumer_id,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_release_profile(
    app: *mut NmpApp,
    pubkey: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    app.send_cmd(ActorCommand::ReleaseProfile {
        pubkey,
        consumer_id,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_close_author(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    app.send_cmd(ActorCommand::CloseAuthor { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_close_thread(app: *mut NmpApp, event_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(event_id) = c_string_argument(event_id) else {
        return;
    };
    if !is_hex_id(&event_id) {
        return;
    }

    app.send_cmd(ActorCommand::CloseThread { event_id });
}
