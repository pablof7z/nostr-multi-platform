//! C FFI surface for the broker.
//!
//! Two entry points:
//! - `nmp_signer_broker_init(app)` — construct the broker, register the
//!   `bunker://` hook with `nmp-core`. Called once after `nmp_app_new`.
//! - `nmp_app_cancel_bunker_handshake(app)` — abort an in-flight handshake.
//!
//! Both follow the existing `nmp_core::ffi` shape: `extern "C"` taking a
//! `*mut NmpApp`; null pointers are silent no-ops (D6).

use std::sync::{Arc, OnceLock};

use nmp_core::{register_bunker_hook, BunkerHookRequest, NmpApp, NOSTRCONNECT_DEFAULT_RELAY_URL};

use crate::broker::BunkerBroker;

// Allow `unsafe` only in this module — the `*mut NmpApp` deref cannot be
// `unsafe` at the function signature level (the symbol is `extern "C"`).
// The crate-level `#![deny(unsafe_code)]` still applies everywhere else.
#[allow(unsafe_code)]
mod unsafe_app_ref {
    use nmp_core::NmpApp;
    /// Convert a raw `*mut NmpApp` (from the Swift bridge) into a `&NmpApp`
    /// if non-null. SAFETY: the caller guarantees the pointer was produced
    /// by `nmp_app_new()` and has not been freed.
    pub fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
        if app.is_null() {
            None
        } else {
            // SAFETY: caller-guaranteed pointer contract.
            Some(unsafe { &*app })
        }
    }
}

/// Process-global broker handle. The bunker hook closure also holds a
/// strong `Arc<BunkerBroker>`; this `OnceLock` exists so the cancel symbol
/// can reach the broker without a second registration mechanism.
static GLOBAL_BROKER: OnceLock<Arc<BunkerBroker>> = OnceLock::new();

/// Initialise the NIP-46 broker. After this call, any `nmp_app_signin_bunker`
/// dispatch from the Swift / Kotlin shells will route through the broker's
/// handshake state machine. Idempotent — repeated calls after the first do
/// nothing (the broker is process-global).
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()` and not yet
/// freed via `nmp_app_free`. Passing null is safe — the function becomes a
/// silent no-op (D6).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[allow(unsafe_code)] // `#[no_mangle]` is itself flagged by `deny(unsafe_code)`
#[no_mangle]
pub extern "C" fn nmp_signer_broker_init(app: *mut NmpApp) {
    let Some(app) = unsafe_app_ref::app_ref(app) else {
        return;
    };
    if GLOBAL_BROKER.get().is_some() {
        // Already initialised (e.g., a hot-reload scenario). The hook is
        // already registered against the prior broker; replacing it would
        // race with in-flight URIs. Keep the existing broker.
        return;
    }
    let tx = app.actor_sender();
    let broker = BunkerBroker::new(tx);
    let broker_for_hook = Arc::clone(&broker);
    register_bunker_hook(Arc::new(move |request| match request {
        BunkerHookRequest::Connect { uri } => broker_for_hook.start_handshake(uri),
        BunkerHookRequest::Restore { payload_json } => {
            broker_for_hook.restore_session(payload_json);
        }
    }));
    let _ = GLOBAL_BROKER.set(broker);
}

/// Cancel an in-flight bunker handshake, if any. Idempotent / safe to call
/// when nothing is in flight (no-op).
///
/// # Safety
///
/// `app` must be a valid pointer returned by `nmp_app_new()`. Passing null
/// is safe (the `app` arg is currently unused — kept in the signature for
/// future per-app brokers).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[allow(unsafe_code)] // `#[no_mangle]` is itself flagged by `deny(unsafe_code)`
#[no_mangle]
pub extern "C" fn nmp_app_cancel_bunker_handshake(_app: *mut NmpApp) {
    if let Some(broker) = GLOBAL_BROKER.get() {
        broker.cancel();
    }
}

/// Return a freshly generated `nostrconnect://` URI string. The caller MUST
/// free the returned pointer via `nmp_broker_free_string`. Returns null if
/// the broker is not yet initialised (i.e. `nmp_signer_broker_init` has not
/// been called) or if the string contains interior NUL bytes (impossible in
/// practice but guarded for D6).
///
/// `relay_url` may be null; if so, Rust selects the first configured
/// write-capable relay from the app kernel. If the app is null or has no
/// write relay, [`NOSTRCONNECT_DEFAULT_RELAY_URL`] is used.
///
/// `callback_scheme` may be null. When non-null, Rust appends a
/// `&callback=<percent-encoded callback_scheme>` query parameter to the
/// generated `nostrconnect://` URI so the signer app can deep-link back to
/// the originating host. The percent-encoding is performed in Rust — hosts
/// MUST NOT mash the callback onto the URI themselves. This keeps the
/// substrate-owned wire string fully owned by Rust (aim.md §4.6: native code
/// supplies platform capabilities; Rust composes protocol strings).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn nmp_app_nostrconnect_uri(
    app: *mut NmpApp,
    relay_url: *const std::os::raw::c_char,
    callback_scheme: *const std::os::raw::c_char,
) -> *mut std::os::raw::c_char {
    let relay = relay_url_from_arg_or_app(app, relay_url);
    let callback: Option<&str> = if callback_scheme.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null => valid C string for the call duration.
        // An invalid UTF-8 callback is treated as "no callback" rather than
        // panicking across the FFI (D6 — errors become state).
        match unsafe { std::ffi::CStr::from_ptr(callback_scheme).to_str() } {
            Ok(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    };
    let Some(broker) = GLOBAL_BROKER.get() else {
        return std::ptr::null_mut();
    };
    let mut uri = broker.start_nostrconnect_handshake(relay);
    if let Some(scheme) = callback {
        let encoded = crate::uri_encode::percent_encode_query_value(scheme);
        uri.push_str("&callback=");
        uri.push_str(&encoded);
    }
    match std::ffi::CString::new(uri) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[allow(unsafe_code)]
fn relay_url_from_arg_or_app(app: *mut NmpApp, relay_url: *const std::os::raw::c_char) -> String {
    if !relay_url.is_null() {
        // SAFETY: caller guarantees non-null => valid C string for the call duration.
        if let Ok(relay) = unsafe { std::ffi::CStr::from_ptr(relay_url).to_str() } {
            if !relay.is_empty() {
                return relay.to_string();
            }
        }
    }
    unsafe_app_ref::app_ref(app)
        .map(NmpApp::nostrconnect_relay_url)
        .unwrap_or_else(|| NOSTRCONNECT_DEFAULT_RELAY_URL.to_string())
}

/// Free a string returned by `nmp_app_nostrconnect_uri`. Null-safe (no-op).
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_broker_free_string(ptr: *mut std::os::raw::c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was created by CString::into_raw() in this module.
    unsafe { drop(std::ffi::CString::from_raw(ptr)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    // Percent-encoding coverage now lives in `crate::uri_encode::tests`;
    // these tests cover only the FFI-specific argument plumbing.

    #[test]
    fn explicit_relay_arg_still_overrides_kernel_selection() {
        let relay = CString::new("wss://explicit.example").expect("valid CString");

        assert_eq!(
            relay_url_from_arg_or_app(std::ptr::null_mut(), relay.as_ptr()),
            "wss://explicit.example"
        );
    }

    #[test]
    fn null_app_null_relay_uses_core_default() {
        assert_eq!(
            relay_url_from_arg_or_app(std::ptr::null_mut(), std::ptr::null()),
            NOSTRCONNECT_DEFAULT_RELAY_URL
        );
    }
}
