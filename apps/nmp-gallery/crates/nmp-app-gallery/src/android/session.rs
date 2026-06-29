//! Session state and callback trampolines for the Android JNI shim.
//!
//! Split from `android.rs` to stay within the 500-LOC hard cap (file-size
//! doctrine). Contains:
//!
//! * [`GallerySession`] — per-session state (kernel handle, JNI push-listener
//!   slots, host-side ref mirrors).
//! * [`on_update`] and [`handle_capability_request`] — callbacks registered
//!   with the kernel.
//! * Small helpers: [`session_ref`], [`jstring_to_cstring`].

use std::ffi::CString;
use std::sync::{Arc, Mutex};

use jni::objects::{JObject, JString};
use jni::sys::jlong;
use jni::JNIEnv;

use nmp_core::__ffi_internal::capability_error_envelope;
use nmp_native_runtime::NmpApp;

use crate::android_push::{
    GalleryUpdateCtx, GalleryUpdateListener, SignerRequestListenerSlot, SignerRequestPushListener,
};

/// Owns the kernel handle and the boxed update-callback context (which holds
/// the JNI push listener). Freed exactly once in `nativeFree`.
pub(crate) struct GallerySession {
    pub(crate) app: NmpApp,
    /// Owns the JNI push listener slot (issue #614 — D8 no-polling).
    pub(crate) update_ctx: Arc<GalleryUpdateCtx>,
    /// Issue #1612 / ADR-0048 Stage 2 — push listener slot for outbound NIP-55
    /// `ExternalSignerRequest` JSON payloads (D8 no-polling; replaces the
    /// deleted `nativeNextSignerRequest` blocking/timeout drain).
    pub(crate) signer_listener: SignerRequestListenerSlot,
    /// ADR-0063 (#1671) — host-side mirrors of the kernel's `refs.profile` and
    /// `refs.event` row-delta projections. Merged across frames in
    /// `nativeDecodeSnapshotJson` (sidecars carry only changed/cleared rows).
    /// The sole app-side ref stores (D4). Wrapped in a `Mutex` because decode
    /// runs on whichever thread Kotlin drains the push frame on.
    pub(crate) ref_stores: Mutex<crate::GalleryRefStores>,
}

// SAFETY: GallerySession is transferred to Kotlin as a jlong handle; access
// is serialised by the Kotlin caller (nativeNew → nativeFree lifecycle).
unsafe impl Send for GallerySession {}

/// Update callback — runs on the kernel's listener thread. Forwards the
/// borrowed FlatBuffers frame straight to the registered JNI push listener
/// (issue #614 — D8 no-polling).
pub(crate) fn on_update(ctx: &GalleryUpdateCtx, frame: &[u8]) {
    ctx.push(frame);
}

/// Capability handler (ADR-0048 Stage 2 / issue #1612 — D8 no-polling).
///
/// For `external_signer` requests: snapshots the registered push listener
/// under the slot lock, drops the lock, then pushes the payload JSON directly
/// to Kotlin via `onSignerRequest`. Returns `{"status":"dispatched"}` on
/// success; `{"status":"session-closed"}` when no listener is registered (D6).
///
/// For all other namespaces: returns the same error envelope a missing handler
/// would (no Android keyring capability exists in the gallery).
pub(crate) fn handle_capability_request(slot: &SignerRequestListenerSlot, request: &str) -> String {
    let parsed: serde_json::Value = serde_json::from_str(&request).unwrap_or_default();
    let namespace = parsed
        .get("namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if namespace != "external_signer" {
        return capability_error_envelope(request, "unsupported-on-android");
    }
    let Some(payload) = parsed.get("payload_json").and_then(|v| v.as_str()) else {
        return capability_error_envelope(request, "missing-payload");
    };
    let correlation_id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let listener_snapshot: Option<Arc<SignerRequestPushListener>> =
        slot.lock().ok().and_then(|g| g.clone());

    if let Some(listener) = listener_snapshot {
        listener.push(payload);
    } else {
        return capability_error_envelope(request, "session-closed");
    }

    let envelope = serde_json::json!({
        "namespace": "external_signer",
        "correlation_id": correlation_id,
        "result_json": r#"{"status":"dispatched"}"#,
    });
    envelope.to_string()
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
    // Quiescence gate: both runtime slots clear before listener state drops, so
    // context pointers and `GlobalRef`s cannot be dropped during a live push.
    session.app.set_update_listener(None);
    session.app.capability_callback_slot().clear();
    session.app.shutdown();
    // signer_listener Arc drops here, taking the listener GlobalRef with it.
}

/// Register (or clear) the JNI push listener for kernel update frames on the
/// given session's update context.
///
/// `listener` must implement `fun onUpdate(frame: ByteArray)`. Pass `null` to
/// deregister. D6: any JNI failure is a silent no-op.
pub(crate) fn set_update_listener(env: JNIEnv, session: &GallerySession, listener: JObject) {
    if listener.is_null() {
        session.update_ctx.clear_listener();
        return;
    }
    let Ok(vm) = env.get_java_vm() else { return };
    let Ok(global) = env.new_global_ref(&listener) else {
        return;
    };
    session
        .update_ctx
        .set_listener(GalleryUpdateListener::new(vm, global));
}
