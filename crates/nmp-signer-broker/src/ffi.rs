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

use nmp_core::{register_bunker_hook, NmpApp};

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
    register_bunker_hook(Arc::new(move |uri| {
        broker_for_hook.start_handshake(uri);
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
/// `relay_url` may be null; if so, `wss://relay.damus.io` is used as the
/// default relay.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn nmp_app_nostrconnect_uri(
    _app: *mut NmpApp,
    relay_url: *const std::os::raw::c_char,
) -> *mut std::os::raw::c_char {
    let relay: &str = if relay_url.is_null() {
        "wss://relay.damus.io"
    } else {
        // SAFETY: caller guarantees non-null => valid C string for the call duration.
        match unsafe { std::ffi::CStr::from_ptr(relay_url).to_str() } {
            Ok(s) => s,
            Err(_) => "wss://relay.damus.io",
        }
    };
    let Some(broker) = GLOBAL_BROKER.get() else {
        return std::ptr::null_mut();
    };
    let uri = broker.start_nostrconnect_handshake(relay.to_string());
    match std::ffi::CString::new(uri) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
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
