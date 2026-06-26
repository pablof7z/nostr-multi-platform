//! NIP-46 actor-lane C-ABI adapter (PR-B2: broker deleted).
//!
//! All three public C symbols are preserved with identical names and
//! signatures; only the implementation changes — from `BunkerBroker` to the
//! actor-lane runtime (`register_nip46` + `Nip46RuntimeHandle`).
//!
//! ## Design
//!
//! `nmp_signer_broker_init` is the **config-phase** entry point:
//!
//! 1. Calls `ensure_prestart_config` to guard against post-start calls.
//! 2. Calls `register_nip46(app, tx)` to install the `Nip46Interceptor` and
//!    `Nip46ConnectedHook` on the app's substrate registrar slots. Returns a
//!    `Nip46RuntimeHandle` stored on the `NmpApp`.
//! 3. Installs a bunker hook that the actor calls on `StartBunkerHandshake`:
//!    - `Connect { uri }` → `init_bunker` + `deliver_init_effects`.
//!    - `Restore { payload_json }` → `restore_nip46_from_payload`.
//!
//! `nmp_app_cancel_bunker_handshake` calls `cancel_nip46_session` (clears the
//! runtime + posts `UnregisterPersistentSub` for each relay).
//!
//! `nmp_app_nostrconnect_uri` calls `init_nostrconnect` + `deliver_init_effects`
//! and returns the `nostrconnect://` URI to the caller synchronously.
//!
//! ## Layer cleanliness
//!
//! `nmp-ffi` does NOT name `RelayRole` or `ActorLaneTransport` — those details
//! are encapsulated inside `nmp-nip46-runtime::ffi_support`.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Arc;

use nmp_core::BunkerHookRequest;
use nmp_nip46::percent_encode_query_value;

/// Wall-clock Unix seconds for NIP-44 timestamps.  Must not block or touch
/// the actor; all callers are on the hook thread (D8-safe).
fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
use nmp_nip46_runtime::{
    cancel_nip46_session, deliver_init_effects, init_bunker, init_nostrconnect,
    make_sub_id, register_nip46, restore_nip46_from_payload, Nip46RuntimeHandle,
};
use nmp_signers::parse_bunker_uri;
use nostr::{Keys, PublicKey};

use super::{app_ref, NmpApp, NmpConfigStatus};

// ─── nmp_signer_broker_init ──────────────────────────────────────────────────

/// Initialise the NIP-46 actor-lane runtime for `app`.
///
/// After this call, any `nmp_app_signin_bunker` dispatch routes through the
/// actor-lane runtime's handshake state machine. **Idempotent and
/// first-writer-wins per app:** a second pre-start call is a no-op that
/// returns [`NmpConfigStatus::Ok`] without re-registering the interceptor /
/// connected hook / bunker hook (the substrate slots accumulate, so a blind
/// re-register would install duplicate hooks). This matches the old
/// `signer_broker_get_or_init` first-writer semantics. A call after
/// `nmp_app_start` returns [`NmpConfigStatus::AlreadyStarted`].
///
/// ADR-0052 §D3 — the runtime handle and the bunker hook are **per-app** (no
/// process-global), so two `NmpApp`s in one process have independent runtimes
/// and a freed-then-recreated app re-initialises cleanly.
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()` and not yet
/// freed via `nmp_app_free`. Passing null is safe: returns
/// [`NmpConfigStatus::NullApp`].
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_signer_broker_init(app: *mut NmpApp) -> u32 {
    let Some(app) = app_ref(app) else {
        return NmpConfigStatus::NullApp.code();
    };
    if let Err(status) =
        app.ensure_prestart_config("signer_broker", "bunker_hook", "nmp_signer_broker_init")
    {
        return status.code();
    }

    // First-writer-wins: if the runtime is already installed, this is a
    // duplicate pre-start init. Return Ok WITHOUT re-registering — a second
    // register_nip46 + install_bunker_hook would accumulate duplicate
    // interceptors/hooks in the substrate slots (the old broker path was
    // get-or-init / idempotent).
    if app
        .nip46_runtime
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
    {
        return NmpConfigStatus::Ok.code();
    }

    let tx = app.actor_sender();

    // Config-phase wiring: install the interceptor + connected hook.
    // `register_nip46` takes `&impl` because `NmpApp`'s registrar impls use
    // `&self` interior mutability — `&mut *app` would violate aliasing rules.
    let handle = register_nip46(app, tx.clone());

    // Store the handle on the app for cancel / nostrconnect-uri access.
    *app.nip46_runtime
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle.clone());

    // Install the per-app bunker hook. The actor calls this when it receives
    // `StartBunkerHandshake`; the hook runs synchronously on the actor thread
    // (D8 — no blocking, no I/O; effects posted as actor commands).
    let handle_for_hook = handle;
    let tx_for_hook = tx.clone();
    app.install_bunker_hook(Arc::new(move |request| {
        match request {
            BunkerHookRequest::Connect { uri } => {
                start_bunker_connect(&handle_for_hook, &tx_for_hook, uri);
            }
            BunkerHookRequest::Restore { payload_json } => {
                let now = now_unix_secs();
                if let Err(e) = restore_nip46_from_payload(
                    &handle_for_hook,
                    &payload_json,
                    tx_for_hook.clone(),
                    now,
                ) {
                    tracing::warn!(error = %e, "nip46-ffi: restore from payload failed");
                }
            }
        }
    }));

    NmpConfigStatus::Ok.code()
}

/// Parse `uri` as a `bunker://` URI, initialise the runtime, and deliver the
/// initial effects as actor commands.  Called synchronously on the actor thread
/// from the bunker hook — must be fast and non-blocking.
fn start_bunker_connect(
    handle: &Nip46RuntimeHandle,
    sender: &nmp_core::CommandSender,
    uri: String,
) {
    let parsed = match parse_bunker_uri(&uri) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %format!("{e:?}"), "nip46-ffi: bad bunker URI");
            sender.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some(format!("invalid bunker URI: {e:?}")),
            );
            return;
        }
    };
    let local_keys = Keys::generate();
    let sub_id = make_sub_id(local_keys.public_key());
    let remote_pubkey = match PublicKey::from_hex(&parsed.remote_pubkey_hex) {
        Ok(pk) => pk,
        Err(e) => {
            tracing::warn!(error = %e, "nip46-ffi: bad remote pubkey in bunker URI");
            sender.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some(format!("invalid remote pubkey: {e}")),
            );
            return;
        }
    };
    let relay_urls = parsed.relays.clone();
    let secret: Option<String> = parsed.secret.as_ref().map(|zs| zs.as_str().to_string());
    let perms = parsed.permissions.clone();
    let now = now_unix_secs();

    match init_bunker(
        handle,
        sub_id,
        local_keys,
        remote_pubkey,
        relay_urls,
        secret.as_deref(),
        perms.as_deref(),
        now,
    ) {
        Ok(effects) => deliver_init_effects(effects, sender),
        Err(e) => {
            tracing::warn!(error = %e, "nip46-ffi: init_bunker failed");
            sender.bunker_handshake_progress(
                "failed".to_string(),
                None,
                Some(format!("bunker init error: {e}")),
            );
        }
    }
}

// ─── nmp_app_cancel_bunker_handshake ─────────────────────────────────────────

/// Cancel an in-flight bunker handshake, if any. Idempotent and null-safe.
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()`. Passing null is
/// safe. ADR-0052 §D3 — reads THIS app's per-app runtime handle (no
/// process-global).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_cancel_bunker_handshake(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else { return };
    let handle = app.nip46_runtime
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let Some(handle) = handle else { return };
    let tx = app.actor_sender();
    cancel_nip46_session(&handle, &tx);
}

// ─── nmp_app_nostrconnect_uri ─────────────────────────────────────────────────

/// Return a freshly generated `nostrconnect://` URI string. The caller must
/// free the returned pointer via `nmp_free_string`. Returns null if the
/// runtime is not yet initialised, no write relay is configured, or string
/// allocation fails.
///
/// D3: relay selection is Rust-owned — the URI embeds the first
/// write-capable relay from the kernel's relay config
/// (`NmpApp::nostrconnect_relay_url`). The caller supplies only optional
/// platform callback information; it does not choose the relay.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_nostrconnect_uri(
    app: *mut NmpApp,
    callback_scheme: *const c_char,
) -> *mut c_char {
    // D3 / V-65: relay is always Rust-chosen; there is no caller override.
    let Some(relay) = app_ref(app).and_then(NmpApp::nostrconnect_relay_url) else {
        return std::ptr::null_mut();
    };
    let callback: Option<String> = if callback_scheme.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null means a valid C string.
        match unsafe { CStr::from_ptr(callback_scheme).to_str() } {
            Ok(s) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        }
    };

    let Some(app) = app_ref(app) else {
        return std::ptr::null_mut();
    };

    // Get the runtime handle — must have been initialised by nmp_signer_broker_init.
    let handle = {
        let guard = app.nip46_runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.clone()
    };
    let Some(handle) = handle else {
        tracing::warn!("nmp_app_nostrconnect_uri: called before nmp_signer_broker_init");
        return std::ptr::null_mut();
    };

    let perms = NmpApp::nostrconnect_perms(app);

    // Generate ephemeral local keypair + random secret (first 16 hex chars of
    // a freshly-generated ephemeral pubkey = 8 bytes / 64 bits of entropy).
    let local_keys = Keys::generate();
    let sub_id = make_sub_id(local_keys.public_key());
    let expected_secret: String = Keys::generate().public_key().to_hex()[..16].to_string();

    let tx = app.actor_sender();
    let now = now_unix_secs();

    match init_nostrconnect(
        &handle,
        sub_id,
        local_keys,
        relay,
        expected_secret,
        perms,
        "nmp",
        now,
    ) {
        Ok((mut uri, effects)) => {
            deliver_init_effects(effects, &tx);
            if let Some(scheme) = callback {
                uri.push_str("&callback=");
                uri.push_str(&percent_encode_query_value(&scheme));
            }
            match CString::new(uri) {
                Ok(cs) => cs.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "nmp_app_nostrconnect_uri: init_nostrconnect failed");
            std::ptr::null_mut()
        }
    }
}
