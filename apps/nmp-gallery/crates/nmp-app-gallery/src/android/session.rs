//! Session state and callback trampolines for the Android JNI shim.
//!
//! Split from `android.rs` to stay within the 500-LOC hard cap (file-size
//! doctrine). Contains:
//!
//! * [`GallerySession`] — per-session state (kernel handle, JNI push-listener
//!   slots, host-side ref-profile mirror).
//! * [`on_update`] / [`on_capability_request`] — the two `extern "C"`
//!   trampolines registered with the kernel.
//! * Small helpers: [`to_c_string`], [`session_ref`], [`jstring_to_cstring`].

use std::ffi::{CStr, CString, c_void};
use std::sync::{Arc, Mutex};

use jni::JNIEnv;
use jni::objects::{JObject, JString};
use jni::sys::jlong;

use nmp_ffi::{
    nmp_app_set_capability_callback, nmp_app_set_update_callback, NmpApp,
};

use nmp_core::refs::RefProfileStore;
use nmp_core::__ffi_internal::capability_error_envelope;

use crate::android_push::{
    GalleryUpdateCtx, GalleryUpdateListener, SignerRequestListenerSlot, SignerRequestPushListener,
};

/// Owns the kernel handle and the boxed update-callback context (which holds
/// the JNI push listener). Freed exactly once in `nativeFree`.
pub(crate) struct GallerySession {
    pub(crate) app: *mut NmpApp,
    /// Boxed [`GalleryUpdateCtx`] passed as the `nmp_app_set_update_callback`
    /// context. Owns the JNI push listener slot (issue #614 — D8 no-polling).
    pub(crate) update_ctx: *mut GalleryUpdateCtx,
    /// Issue #1612 / ADR-0048 Stage 2 — push listener slot for outbound NIP-55
    /// `ExternalSignerRequest` JSON payloads (D8 no-polling; replaces the
    /// deleted `nativeNextSignerRequest` blocking/timeout drain).
    pub(crate) signer_listener: SignerRequestListenerSlot,
    /// ADR-0063 (#1671) — host-side mirror of the kernel's `refs.profile`
    /// row-delta projection. Merged across frames in `nativeDecodeSnapshotJson`
    /// (the sidecar carries only changed/cleared rows). The sole app-side profile
    /// store (D4). Wrapped in a `Mutex` because the decode runs on whichever
    /// thread Kotlin drains the push frame on.
    pub(crate) ref_profiles: Mutex<RefProfileStore>,
}

// SAFETY: GallerySession is transferred to Kotlin as a jlong handle; access
// is serialised by the Kotlin caller (nativeNew → nativeFree lifecycle).
unsafe impl Send for GallerySession {}

/// Update callback — runs on the kernel's listener thread. Forwards the
/// borrowed FlatBuffers frame straight to the registered JNI push listener
/// (issue #614 — D8 no-polling).
pub(crate) extern "C" fn on_update(context: *mut c_void, bytes: *const u8, len: usize) {
    if context.is_null() || bytes.is_null() {
        return;
    }
    let ctx = unsafe { &*(context as *const GalleryUpdateCtx) };
    let frame = unsafe { std::slice::from_raw_parts(bytes, len) };
    ctx.push(frame);
}

/// Capability trampoline (ADR-0048 Stage 2 / issue #1612 — D8 no-polling).
///
/// For `external_signer` requests: snapshots the registered push listener
/// under the slot lock, drops the lock, then pushes the payload JSON directly
/// to Kotlin via `onSignerRequest`. Returns `{"status":"dispatched"}` on
/// success; `{"status":"session-closed"}` when no listener is registered (D6).
///
/// For all other namespaces: returns the same error envelope a missing handler
/// would (no Android keyring capability exists in the gallery).
///
/// Context is a raw pointer to the `Mutex` inside the `Arc<Mutex<...>>` slot
/// stored in `GallerySession`. The `Arc` keeps the allocation alive for the
/// full session lifetime. `nativeFree` calls
/// `nmp_app_set_capability_callback(…, None)` (which blocks until any
/// in-flight call returns) BEFORE the session is dropped, so dereferencing
/// this pointer here is safe.
pub(crate) extern "C" fn on_capability_request(
    context: *mut c_void,
    request_json: *const std::os::raw::c_char,
) -> *mut std::os::raw::c_char {
    if context.is_null() || request_json.is_null() {
        return std::ptr::null_mut();
    }
    let request = unsafe { CStr::from_ptr(request_json) }
        .to_string_lossy()
        .into_owned();
    let parsed: serde_json::Value = serde_json::from_str(&request).unwrap_or_default();
    let namespace = parsed.get("namespace").and_then(|v| v.as_str()).unwrap_or("");
    if namespace != "external_signer" {
        return to_c_string(capability_error_envelope(&request, "unsupported-on-android"));
    }
    let Some(payload) = parsed.get("payload_json").and_then(|v| v.as_str()) else {
        return to_c_string(capability_error_envelope(&request, "missing-payload"));
    };
    let correlation_id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // SAFETY: context points into the Arc<Mutex<...>> stored in GallerySession;
    // lifetime guaranteed by the `nativeFree` quiescence ordering (see module
    // doc and on_capability_request's own doc comment above).
    let slot = unsafe {
        &*(context as *const std::sync::Mutex<Option<Arc<SignerRequestPushListener>>>)
    };
    let listener_snapshot: Option<Arc<SignerRequestPushListener>> =
        slot.lock().ok().and_then(|g| g.clone());

    if let Some(listener) = listener_snapshot {
        listener.push(payload);
    } else {
        return to_c_string(capability_error_envelope(&request, "session-closed"));
    }

    let envelope = serde_json::json!({
        "namespace": "external_signer",
        "correlation_id": correlation_id,
        "result_json": r#"{"status":"dispatched"}"#,
    });
    to_c_string(envelope.to_string())
}

pub(crate) fn to_c_string(value: String) -> *mut std::os::raw::c_char {
    CString::new(value)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

pub(crate) fn session_ref<'a>(handle: jlong) -> Option<&'a GallerySession> {
    if handle == 0 {
        None
    } else {
        Some(unsafe { &*(handle as *const GallerySession) })
    }
}

pub(crate) fn jstring_to_cstring(env: &mut JNIEnv, value: &JString) -> Option<CString> {
    let s = env.get_string(value).ok()?;
    CString::new(s.to_string_lossy().into_owned()).ok()
}

/// Quiescence teardown: deregisters both callbacks (blocking until any
/// in-flight trampoline returns) then frees the kernel handle and the
/// update-callback context. Called from `nativeFree`; must run before the
/// `GallerySession` is dropped so the context pointers remain valid for any
/// concurrent trampoline execution.
///
/// # Safety
/// `session` must be a `Box`-owned `GallerySession` whose `app` and
/// `update_ctx` were set up by `nativeNew`. The caller must not use
/// `session` after this call.
pub(crate) unsafe fn teardown_session(session: Box<GallerySession>) {
    unsafe {
        // Quiescence gate: both `set_*_callback(None)` calls block until any
        // in-flight trampoline returns, so the context pointers and the
        // `GlobalRef` inside the signer listener can be dropped safely without
        // a UAF.
        nmp_app_set_update_callback(session.app, std::ptr::null_mut(), None);
        nmp_app_set_capability_callback(session.app, std::ptr::null_mut(), None);
        nmp_ffi::nmp_app_free(session.app);
        drop(Box::from_raw(session.update_ctx));
        // signer_listener Arc drops here, taking the listener GlobalRef with it.
    }
}

/// Register (or clear) the JNI push listener for kernel update frames on the
/// given session's update context.
///
/// `listener` must implement `fun onUpdate(frame: ByteArray)`. Pass `null` to
/// deregister. D6: any JNI failure is a silent no-op.
pub(crate) fn set_update_listener(
    env: JNIEnv,
    session: &GallerySession,
    listener: JObject,
) {
    let ctx = unsafe { &*session.update_ctx };
    if listener.is_null() {
        ctx.clear_listener();
        return;
    }
    let Ok(vm) = env.get_java_vm() else { return };
    let Ok(global) = env.new_global_ref(&listener) else { return };
    ctx.set_listener(GalleryUpdateListener::new(vm, global));
}
