//! Android JNI surface for the NIP-55 external-signer capability
//! (ADR-0048 Stage 2).
//!
//! Three pieces, mirroring the update-frame JNI push architecture (issue
//! #1284 — D8 no-polling; the request path is a push callback, not a blocking
//! timed drain):
//!
//! 1. **Capability trampoline** — registered with the kernel's capability
//!    socket at `nativeNew`. When the Rust NIP-55 driver dispatches a
//!    `CapabilityRequest { namespace: "external_signer" }`, the trampoline
//!    pushes the inner `ExternalSignerRequest` JSON straight to the registered
//!    Kotlin signer-request listener and acks with `{"status":"dispatched"}`.
//!    Non-`external_signer` namespaces return the same error envelope a
//!    missing handler would (no Android keyring capability exists yet).
//! 2. **`nativeSetSignerRequestListener` / `nativeClearSignerRequestListener`**
//!    (in `lib.rs`) — register the Kotlin `onSignerRequest(requestJson)`
//!    callback the trampoline pushes to, mirroring `nativeSetUpdateListener`.
//! 3. **`nativeSignInNip55` / `nativeDeliverSignerResponse`** — user intent
//!    in, raw results back (D7: Kotlin decides nothing).
//!
//! The trampoline `context` is the **session registry handle id**, not a
//! raw pointer: a dispatch racing session teardown degrades to an error
//! envelope via the registry lookup (D6), never a use-after-free.

use std::ffi::{c_char, c_void, CString};
use std::mem::size_of;
use std::ptr;

use jni::objects::{JClass, JObject, JString};
use jni::sys::jlong;
use jni::JNIEnv;

use nmp_core::__ffi_internal::capability_error_envelope;
use nmp_ffi::{
    nmp_app_deliver_external_signer_response, nmp_app_set_capability_callback,
    nmp_app_signin_nip55, nmp_external_signer_init, NmpApp,
};

use crate::{jstring_to_cstring, session_arc};

/// Wire constant — must match `nmp_signer_iface::EXTERNAL_SIGNER_NAMESPACE`.
/// (The JNI crate lives in a detached workspace and does not depend on
/// `nmp-signer-iface`; the namespace is part of the stable capability wire.)
const EXTERNAL_SIGNER_NAMESPACE: &str = "external_signer";

/// Register the capability trampoline + initialise the NIP-55 driver for a
/// freshly created session. Called from `nativeNew` AFTER `insert_session`
/// assigned `handle` (the trampoline context is the handle id).
pub(crate) fn install(app: *mut NmpApp, handle: jlong) {
    // Defensive: `jlong` is i64; we store it as a `usize` in the trampoline
    // context pointer. On 32-bit targets `usize` is 4 bytes and the cast
    // would silently truncate. Android dropped 32-bit ABI support in NDK r21
    // (API 21), so this assertion is always satisfied on real devices — it
    // fires only if someone mistakenly targets a 32-bit host.
    debug_assert!(
        size_of::<usize>() >= size_of::<jlong>(),
        "jlong→usize truncation: target is 32-bit, handle id would be corrupted"
    );
    nmp_app_set_capability_callback(app, handle as usize as *mut c_void, Some(on_capability_request));
    nmp_external_signer_init(app);
}

/// Capability trampoline. Runs on whichever Rust thread dispatches the
/// capability (the NIP-55 driver's caller or the capability worker) — it
/// only parses JSON, pushes onto an mpsc channel, and allocates the ack
/// envelope. Never blocks, never panics across the boundary (D6).
extern "C" fn on_capability_request(
    context: *mut c_void,
    request_json: *const c_char,
) -> *mut c_char {
    if request_json.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: the dispatcher guarantees a valid NUL-terminated string for
    // the duration of the call.
    let request = unsafe { std::ffi::CStr::from_ptr(request_json) }
        .to_string_lossy()
        .into_owned();

    let parsed: serde_json::Value = serde_json::from_str(&request).unwrap_or_default();
    let namespace = parsed.get("namespace").and_then(|v| v.as_str()).unwrap_or("");
    if namespace != EXTERNAL_SIGNER_NAMESPACE {
        // Route all non-external_signer namespaces to the registered
        // synchronous capability handler (e.g. Android Keystore keyring).
        // The trampoline context is the registry handle id.
        let handle = context as usize as jlong;
        if let Some(session) = session_arc(handle) {
            let result = crate::capability::call_sync_handler(&session.capability_handler, &request)
                .unwrap_or_else(|| capability_error_envelope(&request, "no-capability-handler"));
            return to_c_string(result);
        }
        return to_c_string(capability_error_envelope(&request, "session-closed"));
    }
    let correlation_id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let Some(payload) = parsed.get("payload_json").and_then(|v| v.as_str()) else {
        return to_c_string(capability_error_envelope(&request, "missing-payload"));
    };

    let handle = context as usize as jlong;
    // Issue #1284 — push the request JSON straight to the registered Kotlin
    // signer-request listener (D8: no polling). `push_signer_request` returns
    // false when the session is gone OR when no listener is registered yet; both
    // are reported as the same error envelope the driver already handles.
    let pushed = session_arc(handle)
        .map(|session| session.push_signer_request(payload))
        .unwrap_or(false);
    if !pushed {
        return to_c_string(capability_error_envelope(&request, "session-closed"));
    }

    let envelope = serde_json::json!({
        "namespace": EXTERNAL_SIGNER_NAMESPACE,
        "correlation_id": correlation_id,
        "result_json": r#"{"status":"dispatched"}"#,
    });
    to_c_string(envelope.to_string())
}

fn to_c_string(value: String) -> *mut c_char {
    CString::new(value)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

/// Begin a NIP-55 sign-in. `signer_package` may be null ("let the OS
/// resolver pick"); Rust builds the `get_public_key` + permission-batch
/// request (D7 — Kotlin reports user intent only).
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSignInNip55(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    signer_package: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let package = optional_jstring_to_cstring(&mut env, &signer_package);
    s.with_app(|app| {
        nmp_app_signin_nip55(
            app,
            package.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
        )
    });
}

/// Report a raw `ExternalSignerResponse` JSON back to the Rust driver
/// (D7 — verbatim; the driver owns correlation routing and all policy).
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeDeliverSignerResponse(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    response_json: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(response) = jstring_to_cstring(&mut env, &response_json) else {
        return;
    };
    s.with_app(|app| nmp_app_deliver_external_signer_response(app, response.as_ptr()));
}

fn optional_jstring_to_cstring(env: &mut JNIEnv, value: &JString) -> Option<CString> {
    let obj: &JObject = AsRef::<JObject>::as_ref(value);
    if obj.as_raw().is_null() {
        return None;
    }
    jstring_to_cstring(env, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::session::{insert_session, remove_session, Session};

    fn call_trampoline(handle: jlong, request: &str) -> serde_json::Value {
        let request_c = CString::new(request).unwrap();
        let raw = on_capability_request(handle as usize as *mut c_void, request_c.as_ptr());
        assert!(!raw.is_null());
        // SAFETY: trampoline contract — non-null return is a CString we own.
        let owned = unsafe { CString::from_raw(raw) };
        serde_json::from_str(&owned.to_string_lossy()).expect("envelope is JSON")
    }

    #[test]
    fn external_signer_request_is_pushed_to_listener() {
        // Issue #1284: the trampoline pushes the request JSON straight to the
        // registered signer-request listener (D8 — no polling). With a listener
        // present (here the test capture sink standing in for the JVM-backed
        // one), the trampoline acks `dispatched` and the payload is delivered.
        let session = Session::test_session();
        session.arm_signer_request_capture();
        let handle = insert_session(Arc::clone(&session));

        let envelope = call_trampoline(
            handle,
            r#"{"namespace":"external_signer","correlation_id":"c1","payload_json":"{\"correlation_id\":\"c1\",\"method\":\"get_public_key\",\"payload\":\"\"}"}"#,
        );
        assert_eq!(envelope["namespace"], "external_signer");
        assert_eq!(envelope["correlation_id"], "c1");
        assert!(envelope["result_json"]
            .as_str()
            .unwrap()
            .contains("dispatched"));

        let pushed = session.captured_signer_requests();
        assert_eq!(pushed.len(), 1, "exactly one request pushed");
        assert!(
            pushed[0].contains("get_public_key"),
            "pushed payload carries the request method: {}",
            pushed[0]
        );

        remove_session(handle);
    }

    #[test]
    fn external_signer_request_with_no_listener_error_envelopes() {
        // No signer-request listener registered (and no capture sink armed):
        // the push has nowhere to go, so the trampoline reports the same
        // `session-closed` error envelope the driver already handles (D6 —
        // never a panic or NULL). This proves the request never silently
        // vanishes when the host has not yet registered its callback.
        let session = Session::test_session();
        let handle = insert_session(session);

        let envelope = call_trampoline(
            handle,
            r#"{"namespace":"external_signer","correlation_id":"c1","payload_json":"{\"method\":\"get_public_key\"}"}"#,
        );
        assert!(envelope["result_json"]
            .as_str()
            .unwrap()
            .contains("session-closed"));

        remove_session(handle);
    }

    #[test]
    fn non_external_namespace_with_no_handler_returns_error_envelope() {
        // No synchronous capability handler registered — the trampoline
        // degrades to an error envelope (D6: never a panic or NULL).
        let session = Session::test_session();
        let handle = insert_session(session);

        let envelope = call_trampoline(
            handle,
            r#"{"namespace":"nmp.keyring.capability","correlation_id":"c2","payload_json":"{}"}"#,
        );
        // The `call_sync_handler` returns None when the slot is empty, which
        // maps to "no-capability-handler" in the result_json.
        assert!(envelope["result_json"]
            .as_str()
            .unwrap()
            .contains("no-capability-handler"));

        remove_session(handle);
    }

    #[test]
    fn dead_session_returns_error_envelope() {
        let envelope = call_trampoline(
            0,
            r#"{"namespace":"external_signer","correlation_id":"c3","payload_json":"{}"}"#,
        );
        assert!(envelope["result_json"]
            .as_str()
            .unwrap()
            .contains("session-closed"));
    }
}
