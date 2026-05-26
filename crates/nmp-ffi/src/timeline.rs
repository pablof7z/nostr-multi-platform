//! Timeline / profile FFI wrappers — open/close author + thread + firehose,
//! `nostr:` URI routing, and profile claim/release.
//!
//! Split out of `ffi/mod.rs` to keep both files under the 300-LOC soft cap.
//! These reuse the parent module's validated-argument helpers (`app_ref`,
//! `c_string_argument`) and the shared `NmpApp` handle; the symbols stay
//! `#[no_mangle] extern "C"` so the Swift bridge sees a flat C ABI regardless
//! of the Rust module split.

use super::{app_ref, c_string_argument, NmpApp};
use nmp_core::ActorCommand;
use nmp_core::__ffi_internal::{is_hex_id, is_hex_pubkey};
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

    app.send_cmd(ActorCommand::Kernel(nmp_core::KernelAction::OpenUri {
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

/// Claim an embedded event by `nostr:` URI (T180 / ADR-0034). Refcounted
/// per `consumer_id`; the kernel fetches the event over the OneshotApi
/// (single-writer interest registration — D4) when not yet in the store,
/// and surfaces it in snapshot `projections.claimed_events` keyed by
/// `primary_id` (event-id hex for `nevent`/`note`; `"kind:pubkey:d"` for
/// `naddr`). FFI-clean (D6): a null/invalid argument is a silent no-op,
/// never a panic. D8: forwards to the actor; no polling, no sync wait.
#[no_mangle]
pub extern "C" fn nmp_app_claim_event(
    app: *mut NmpApp,
    uri: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };

    app.send_cmd(ActorCommand::ClaimEvent { uri, consumer_id });
}

/// Release a previously-claimed embedded event (T180 / ADR-0034). Mirrors
/// `nmp_app_release_profile`: decrements the per-consumer refcount in the
/// kernel's `event_claims` table; the kernel drops the row when the set
/// is empty. The OneshotApi interest itself is released EOSE-driven via
/// the existing `complete_unknown_oneshot` path. FFI-clean (D6): a null
/// or invalid argument is a silent no-op. D8: forwards to the actor.
#[no_mangle]
pub extern "C" fn nmp_app_release_event(
    app: *mut NmpApp,
    uri: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };

    app.send_cmd(ActorCommand::ReleaseEvent { uri, consumer_id });
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
