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
/// uses to retrieve the result. Caller frees with `nmp_free_string`.
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
/// active account, publishing no metadata — the standard sign-in. Sets
/// `pending_mls_autopublish` so the next `nmp_marmot_register[_active]` call
/// automatically publishes a key package; accounts signed in this way are
/// immediately MLS-capable without the user visiting Settings.
///
/// `make_active = 0`: registers a visible secondary signer without activating
/// it. For hidden app-managed keys, use `nmp_app_register_agent_nsec`.
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
    // Route through `add_signer` so the "active local key ⇒ arm MLS
    // autopublish" rule lives in exactly one place (D4); see `NmpApp::add_signer`.
    app.add_signer(nmp_core::SignerSource::LocalNsec(secret), make_active != 0);
}

/// Register a persisted app-managed local signer.
///
/// The key is signable by explicit pubkey through publish/upload/sign-return
/// paths, but it is hidden from account projections and can never become the
/// active user account.
#[no_mangle]
pub extern "C" fn nmp_app_register_agent_nsec(app: *mut NmpApp, secret: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(secret) = c_string_argument(secret).map(zeroize::Zeroizing::new) else {
        return;
    };
    app.send_cmd(ActorCommand::AddSigner {
        source: nmp_core::SignerSource::AppManagedLocalNsec(secret),
        make_active: false,
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

/// Create a new account (generate keypair, publish kind:0 + kind:10002).
///
/// `make_active = 1`: make the new account active immediately (standard
/// onboarding flow).
/// `make_active = 0`: create the account without switching to it — useful for
/// creating an agent/secondary account alongside an existing active session.
#[no_mangle]
pub extern "C" fn nmp_app_create_new_account(
    app: *mut NmpApp,
    profile_json: *const c_char,
    relays_json: *const c_char,
    mls: bool,
    make_active: u8,
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
        // Generic create-account auto-follows nobody. Auto-follow is operator
        // policy that lives in the leaf app, not in framework FFI (#1493). A
        // Chirp-owned wrapper (`nmp_app_chirp_create_new_account`) injects
        // Chirp's seed follows via `create_new_account_with_initial_follows`.
        initial_follows: Vec::new(),
        mls,
        make_active: make_active != 0,
    });
}

/// Shared create-account dispatch with an explicit, app-supplied initial
/// follow set. Re-exported for app composition crates (e.g. `nmp-app-chirp`)
/// that own the seed-follow policy and must thread it in WITHOUT routing
/// operator pubkeys through the thin native shell (#1493) — the same Rust-owned
/// pattern the relay bootstrap uses (`nmp_app_chirp_seed_default_relays`).
///
/// `follows` is the list of hex pubkeys the fresh account auto-follows. An empty
/// slice means no contacts are prepopulated and no cold-start kind:3 is
/// published. Returns `true` when the command was dispatched, `false` on a null
/// app or undecodable `profile_json` / `relays_json` (D6 — surfaces a toast,
/// never traps across the FFI).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn create_new_account_with_initial_follows(
    app: *mut NmpApp,
    profile_json: *const c_char,
    relays_json: *const c_char,
    mls: bool,
    make_active: u8,
    follows: Vec<String>,
) -> bool {
    let Some(app) = app_ref(app) else {
        return false;
    };
    let Some(profile_json) = c_string_argument(profile_json) else {
        return false;
    };
    let Some(relays_json) = c_string_argument(relays_json) else {
        return false;
    };

    let profile: std::collections::HashMap<String, String> =
        if let Ok(p) = serde_json::from_str(&profile_json) {
            p
        } else {
            app.send_cmd(ActorCommand::ShowToast {
                message: "Failed to decode profile JSON".to_string(),
            });
            return false;
        };

    let relays: Vec<(String, String)> = if let Ok(r) = serde_json::from_str(&relays_json) {
        r
    } else {
        app.send_cmd(ActorCommand::ShowToast {
            message: "Failed to decode relays JSON".to_string(),
        });
        return false;
    };

    app.set_pending_mls_autopublish(mls);
    app.send_cmd(ActorCommand::CreateAccount {
        profile,
        relays,
        initial_follows: follows,
        mls,
        make_active: make_active != 0,
    });
    true
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

// V-68 Stage 2 (ADR-0042 amendment 2026-06-12): `nmp_app_open_timeline` deleted.
// Callers must use the Chirp wrapper `nmp_app_chirp_open_home_feed` (which
// declares primary home-feed kinds in one place). The old generic
// `nmp_app_open_contact_feed`/`nmp_app_close_contact_feed` C symbols are
// legacy shims in `crates/nmp-ffi/src/timeline.rs`.

#[cfg(test)]
mod autopublish_flag_tests {
    //! Verifies that every local-key sign-in path sets `pending_mls_autopublish`
    //! and that the flag is NOT set for non-active or bunker sign-ins.
    use super::*;
    use crate::{nmp_app_free, nmp_app_new};
    use std::ffi::CString;

    /// A stable, valid nsec used across sign-in flag tests.
    const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

    fn nsec_c() -> CString {
        CString::new(TEST_NSEC).unwrap()
    }

    /// `nmp_app_signin_nsec(make_active=1)` must set the autopublish flag so
    /// the next `nmp_marmot_register[_active]` publishes a key package.
    #[test]
    fn signin_nsec_make_active_sets_autopublish_flag() {
        let app = nmp_app_new();
        let app_ref = unsafe { &*app };
        // Precondition: flag starts false.
        assert!(
            !app_ref.take_pending_mls_autopublish(),
            "flag must start false"
        );

        // Set it back; sign in as active.
        nmp_app_signin_nsec(app, nsec_c().as_ptr(), 1);
        assert!(
            app_ref.take_pending_mls_autopublish(),
            "make_active=1 sign-in must set pending_mls_autopublish"
        );

        // Flag must be cleared by take (consume-once semantics).
        assert!(
            !app_ref.take_pending_mls_autopublish(),
            "take_pending_mls_autopublish must be one-shot"
        );
        nmp_app_free(app);
    }

    /// `nmp_app_signin_nsec(make_active=0)` registers a visible secondary key
    /// and must NOT set the autopublish flag (secondary keys are never
    /// registered with Marmot).
    #[test]
    fn signin_nsec_secondary_does_not_set_autopublish_flag() {
        let app = nmp_app_new();
        let app_ref = unsafe { &*app };

        nmp_app_signin_nsec(app, nsec_c().as_ptr(), 0);
        assert!(
            !app_ref.take_pending_mls_autopublish(),
            "make_active=0 sign-in must NOT set pending_mls_autopublish"
        );
        nmp_app_free(app);
    }

    #[test]
    fn register_agent_nsec_does_not_set_autopublish_flag() {
        let app = nmp_app_new();
        let app_ref = unsafe { &*app };

        nmp_app_register_agent_nsec(app, nsec_c().as_ptr());
        assert!(
            !app_ref.take_pending_mls_autopublish(),
            "app-managed local signers are hidden agent keys, not MLS accounts"
        );
        nmp_app_free(app);
    }
}
