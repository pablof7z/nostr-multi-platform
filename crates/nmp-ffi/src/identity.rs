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
use nmp_core::ActorCommand;
use std::ffi::{c_char, CString};

/// Mint a unique correlation id for a `SignEventForReturn` round-trip.
///
/// Same shape and rationale as `nmp-core`'s `new_action_id`: a wall-clock
/// millisecond stamp concatenated with a process-lifetime atomic counter, so
/// two ids minted in the same millisecond still differ. This is a correlation
/// handle, not a security token — no cryptographic randomness is required. The
/// host treats it as an opaque key into the `signed_events` projection.
fn new_sign_return_correlation_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{now_ms:016x}{seq:016x}")
}

/// Sign an event draft and park the signed JSON in the `signed_events`
/// snapshot projection. Returns an opaque `correlation_id` C string the caller
/// uses to retrieve the result. Caller frees with `nmp_app_free_string`.
///
/// This is the D13 sign-and-return seam: a host that needs a signed auth event
/// (e.g. a Blossom upload `Authorization: Nostr …` header, or a feedback
/// event) gets it signed by the kernel's active or named signer WITHOUT ever
/// reading raw private key bytes across the FFI boundary — which is impossible
/// for NIP-46 bunker users. The signed event is NEVER published.
///
/// `account_pubkey_hex` — hex pubkey of the signer to use; pass the empty
/// string to use the active account.
///
/// `unsigned_json` — `{"kind":N,"content":"...","tags":[...],"created_at":N}`.
/// `created_at` is advisory; the kernel re-stamps it from its own clock (D7).
///
/// The host registers a `signed_events`-keyed continuation BEFORE calling this
/// (the return-then-suspend ordering guarantees the id exists before the first
/// projection tick that could carry it). A null `app` returns a freshly minted
/// id whose result will never appear — the caller's continuation must time out
/// (the kernel never saw the command).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_sign_event_for_return(
    app: *mut NmpApp,
    account_pubkey_hex: *const c_char,
    unsigned_json: *const c_char,
) -> *mut c_char {
    let correlation_id = new_sign_return_correlation_id();
    // Mint and return the id regardless of arg validity: the caller suspends on
    // this id, and a malformed/empty draft surfaces as an `Err` verdict under
    // the SAME id once the kernel records it (D6 — never a crash, the promise
    // always resolves). A null `app` is the one case the kernel never sees;
    // the caller's continuation times out.
    if let Some(app) = app_ref(app) {
        let account_pubkey = c_string_argument(account_pubkey_hex).unwrap_or_default();
        let unsigned_json = c_string_argument(unsigned_json).unwrap_or_default();
        app.send_cmd(ActorCommand::SignEventForReturn {
            account_pubkey,
            unsigned_json,
            correlation_id: correlation_id.clone(),
        });
    }
    // The id is plain hex — no interior NUL — so `CString::new` cannot fail in
    // practice; the empty-string fallback keeps the boundary panic-free (D6).
    CString::new(correlation_id)
        .unwrap_or_else(|_| c"".to_owned())
        .into_raw()
}


/// Sign in with a local nsec and optionally make it the active account.
///
/// `make_active = 1` (the common path): registers the signer AND makes it the
/// active account, publishing no metadata — the standard sign-in.
///
/// `make_active = 0`: registers the signer in the kernel's identity roster
/// WITHOUT activating it. The key can then sign events via
/// `nmp_app_sign_event_for_return` by naming its pubkey explicitly. Use this
/// for agent / secondary keys that must sign (e.g. Blossom auth events) without
/// disturbing the user's active account.
///
/// D13: the nsec is wrapped in `Zeroizing` the instant it is copied out of the
/// C string; no raw key bytes are retained past the command dispatch.
#[no_mangle]
pub extern "C" fn nmp_app_signin_nsec(app: *mut NmpApp, secret: *const c_char, make_active: u8) {
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
    app.send_cmd(ActorCommand::AddSigner {
        source: nmp_core::SignerSource::LocalNsec(secret),
        make_active: make_active != 0,
    });
}

/// Connect a NIP-46 bunker signer.
///
/// `make_active = 1`: handshake completes and the resolved pubkey becomes the
/// active account (the normal bunker sign-in path).
///
/// `make_active = 0`: registers the bunker signer WITHOUT activating it once
/// the handshake completes — for agent/secondary keys that sign via
/// `nmp_app_sign_event_for_return` without disturbing the user's active
/// account. The `make_active` flag is carried through the async handshake
/// round-trip by the kernel's signer broker (D0: nmp-core owns the stash).
#[no_mangle]
pub extern "C" fn nmp_app_signin_bunker(app: *mut NmpApp, uri: *const c_char, make_active: u8) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(uri) = c_string_argument(uri) else {
        return;
    };
    app.send_cmd(ActorCommand::AddSigner {
        source: nmp_core::SignerSource::BunkerUri(uri),
        make_active: make_active != 0,
    });
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
        if let Ok(p) = serde_json::from_str(&profile_json) {
            p
        } else {
            app.send_cmd(ActorCommand::ShowToast {
                message: "Failed to decode profile JSON".to_string(),
            });
            return;
        };

    let relays: Vec<(String, String)> = if let Ok(r) = serde_json::from_str(&relays_json) {
        r
    } else {
        app.send_cmd(ActorCommand::ShowToast {
            message: "Failed to decode relays JSON".to_string(),
        });
        return;
    };

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
pub extern "C" fn nmp_app_add_relay(app: *mut NmpApp, url: *const c_char, role: *const c_char) {
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

/// C ABI symbol kept stable (Swift / Kotlin / TUI call it). Internally it now
/// opens the contact-list-authors subscription, declaring Chirp's social
/// timeline kinds {1, 6} — the host-declared kind set that `nmp-core` no longer
/// hardcodes (D0). V-68 Stage 2 (#911): replace with
/// `nmp_app_open_interest(filter_json, consumer_id, scope)` once the ADR is
/// written and merged.
#[no_mangle]
pub extern "C" fn nmp_app_open_timeline(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::OpenContactListSubscription {
        kinds: std::collections::BTreeSet::from([1u32, 6u32]),
    });
}
