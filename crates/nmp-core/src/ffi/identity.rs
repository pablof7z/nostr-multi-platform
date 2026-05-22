//! T66a identity / multi-account / relay-edit FFI wrappers.
//!
//! Split out of `ffi/mod.rs` to keep both files under the 500-LOC hard cap.
//! Publish-handle entry points (signed/unsigned event publish, retry, cancel)
//! used to live alongside the identity ops; they now live in the sibling
//! `ffi/publish.rs` per AGENTS.md "co-locate by owner, not by role". The
//! `#[no_mangle] extern "C"` symbol names stayed byte-stable across that
//! split — the Swift / Android bridge sees the same flat C ABI it always did.
//!
//! These wrappers reuse the parent module's validated-argument helpers
//! (`app_ref`, `c_string_argument`, `c_optional_string_argument`) and the
//! shared `NmpApp` handle; the symbols stay `#[no_mangle] extern "C"` so the
//! Swift bridge sees a flat C ABI regardless of the Rust module split.

use super::{app_ref, c_optional_string_argument, c_string_argument, NmpApp};
use crate::actor::ActorCommand;
use crate::ffi::action::INFLIGHT_DISPATCH_TTL;
use std::ffi::c_char;

#[no_mangle]
pub extern "C" fn nmp_app_signin_nsec(app: *mut NmpApp, secret: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    // Wrap the plaintext nsec in `Zeroizing` the instant it is copied out of
    // the C string. The nsec inevitably crosses the FFI boundary as bytes
    // (it MUST be imported somehow); `Zeroizing` does not eliminate that
    // transit, but it guarantees this Rust-side copy is wiped on drop —
    // including the path where `send_cmd` fails and `secret` is dropped here.
    let Some(secret) = c_string_argument(secret).map(zeroize::Zeroizing::new) else {
        return;
    };
    app.send_cmd(ActorCommand::SignInNsec { secret });
}

#[no_mangle]
pub extern "C" fn nmp_app_signin_bunker(app: *mut NmpApp, uri: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    app.send_cmd(ActorCommand::SignInBunker { uri });
}

#[no_mangle]
pub extern "C" fn nmp_app_create_new_account(
    app: *mut NmpApp,
    profile_json: *const c_char,
    relays_json: *const c_char,
    mls: bool,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(profile_json) = c_string_argument(profile_json) else {
        return;
    };
    let Some(relays_json) = c_string_argument(relays_json) else {
        return;
    };

    let profile: std::collections::HashMap<String, String> =
        match serde_json::from_str(&profile_json) {
            Ok(p) => p,
            Err(_) => {
                app.send_cmd(ActorCommand::ShowToast {
                    message: "Failed to decode profile JSON".to_string(),
                });
                return;
            }
        };

    let relays: Vec<(String, String)> = match serde_json::from_str(&relays_json) {
        Ok(r) => r,
        Err(_) => {
            app.send_cmd(ActorCommand::ShowToast {
                message: "Failed to decode relays JSON".to_string(),
            });
            return;
        }
    };

    // Idempotency guard: two rapid taps mint two distinct keypairs (the second
    // overwrites the first and the user silently loses their nsec). Reject a
    // second dispatch within INFLIGHT_DISPATCH_TTL (30 s) — same TTL as the
    // generic `inflight_dispatches` guard. A poisoned mutex degrades to "let
    // the dispatch through" rather than silently blocking account creation
    // forever (D6: failures are data, not crashes).
    if let Ok(mut slot) = app.creating_account_inflight.lock() {
        let now = std::time::Instant::now();
        if let Some(started) = *slot {
            if now.duration_since(started) < INFLIGHT_DISPATCH_TTL {
                return;
            }
        }
        *slot = Some(now);
    }

    app.set_pending_mls_autopublish(mls);
    app.send_cmd(ActorCommand::CreateAccount {
        profile,
        relays,
        mls,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_switch_active(app: *mut NmpApp, identity_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(identity_id) = c_string_argument(identity_id) else {
        return;
    };
    app.send_cmd(ActorCommand::SwitchActive { identity_id });
}

#[no_mangle]
pub extern "C" fn nmp_app_remove_account(app: *mut NmpApp, identity_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(identity_id) = c_string_argument(identity_id) else {
        return;
    };
    app.send_cmd(ActorCommand::RemoveAccount { identity_id });
}

// `nmp_app_react`, `nmp_app_follow`, `nmp_app_unfollow` were per-verb C
// symbols that sent `ActorCommand::{React,Follow,Unfollow}` directly,
// bypassing the action registry — a D0 violation (social verbs in
// `nmp-core`). They have been deleted: the three social verbs now live in
// `nmp-app-chirp` and reach the kernel through the generic
// `nmp_app_dispatch_action` path under the host-registered `chirp.react` /
// `nmp.follow` / `nmp.unfollow` namespaces (see
// `apps/chirp/nmp-app-chirp/src/ffi.rs::register_chirp_actions`). The
// `ActorCommand` variants themselves stay in `actor/mod.rs` — they are the
// generic command shape the host executors enqueue.

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
    app.send_cmd(ActorCommand::AddRelay { url, role });
}

#[no_mangle]
pub extern "C" fn nmp_app_remove_relay(app: *mut NmpApp, url: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(url) = c_string_argument(url) else {
        return;
    };
    app.send_cmd(ActorCommand::RemoveRelay { url });
}

#[no_mangle]
pub extern "C" fn nmp_app_open_timeline(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::OpenTimeline);
}
