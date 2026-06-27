//! JNI shim: Android ⇄ the nmp-core kernel for the NmpGallery app.
//!
//! Exports `Java_org_nmp_gallery_bridge_KernelBridge_*` symbols matching the
//! `KernelBridge.kt` `external fun` declarations. Pattern mirrors
//! `apps/chirp/crates/nmp-chirp-android-ffi` which does the same for the Chirp app.
//!
//! Doctrine: no business logic or cached state (D5/D8) — pure transport.
//! Errors never cross FFI (D6); outcomes arrive in the next FlatBuffers
//! snapshot frame.
//!
//! Session state, callback trampolines, and small JNI helpers live in the
//! [`session`] submodule (split for file-size compliance).

use std::ffi::c_void;
use std::sync::Arc;

use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_deliver_external_signer_response, nmp_app_new,
    nmp_app_release_profile_ref, nmp_app_resolve_profile_ref, nmp_app_set_capability_callback,
    nmp_app_set_update_callback, nmp_app_signin_nip55, nmp_app_start, nmp_app_stop,
    nmp_external_signer_init,
};

use crate::dispatch_bytes::dispatch_action_bytes_for;

use crate::android_push::{SignerRequestListenerSlot, SignerRequestPushListener};

use std::sync::Mutex;

mod event_refs;
pub(crate) mod session;
use session::{
    jstring_to_cstring, on_capability_request, on_update, session_ref, set_update_listener,
    teardown_session, GallerySession,
};

// ── JNI entry points ──────────────────────────────────────────────────────

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeNew(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    let app = nmp_app_new();
    if app.is_null() {
        return 0;
    }
    // Issue #614 — the update-callback context owns the JNI push listener slot.
    let update_ctx = Box::into_raw(Box::new(crate::android_push::GalleryUpdateCtx::new()));
    nmp_app_set_update_callback(app, update_ctx as *mut c_void, Some(on_update));
    // Issue #1612 / ADR-0048 Stage 2 — push-based signer-request delivery
    // (D8 no-polling; replaces the deleted mpsc-channel + nativeNextSignerRequest drain).
    // The trampoline context is a raw pointer to the Mutex inside the Arc.
    let signer_listener: SignerRequestListenerSlot = Arc::new(Mutex::new(None));
    // SAFETY: the raw pointer into the Arc-owned Mutex outlives any trampoline
    // call because nativeFree calls `nmp_app_set_capability_callback(…, None)`
    // (which blocks until any in-flight call returns) before the session Arc
    // (and thus the Mutex) is dropped. Dereferencing this pointer in
    // `on_capability_request` is safe for the full session lifetime.
    let trampoline_ctx =
        Arc::as_ptr(&signer_listener) as *mut Mutex<Option<Arc<SignerRequestPushListener>>>;
    nmp_app_set_capability_callback(
        app,
        trampoline_ctx as *mut c_void,
        Some(on_capability_request),
    );
    nmp_external_signer_init(app);
    let session = Box::new(GallerySession {
        app,
        update_ctx,
        signer_listener,
        ref_stores: Mutex::new(crate::GalleryRefStores::new()),
    });
    Box::into_raw(session) as jlong
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeFree(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    let s = unsafe { Box::from_raw(handle as *mut GallerySession) };
    unsafe { teardown_session(s) };
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeGalleryRegister(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_ref(handle) {
        crate::nmp_app_gallery_register(s.app as *mut c_void);
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeShowcaseReferencesJson<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
) -> jstring {
    match env.new_string(crate::showcase::raw_json()) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeRegistryJson<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
) -> jstring {
    match env.new_string(crate::registry::raw_json()) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeDecodeSnapshotJson<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    frame: JByteArray<'l>,
) -> jstring {
    let null = std::ptr::null_mut();
    let Ok(bytes) = env.convert_byte_array(frame) else {
        return null;
    };
    // ADR-0063 (#1671): merge the frame's `refs.profile` / `refs.event`
    // row-delta batches into the session's persistent stores before building
    // snapshot JSON. The handle is optional only for the (pre-session) error
    // path; without it row-deltas cannot accumulate, so a missing handle yields
    // null (D6).
    let Some(s) = session_ref(handle) else {
        return null;
    };
    let Ok(mut stores) = s.ref_stores.lock() else {
        return null;
    };
    let Ok(json) = crate::snapshot_json::snapshot_json_from_update_frame(
        &bytes,
        &mut stores.profiles,
        &mut stores.events,
    ) else {
        return null;
    };
    drop(stores);
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => null,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeStart(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    _events_per_sec: jint,
    visible_limit: jint,
    emit_hz: jint,
) {
    let Some(s) = session_ref(handle) else { return };
    for relay in &crate::showcase::references().relays {
        let Ok(url_c) = std::ffi::CString::new(relay.url.as_str()) else {
            continue;
        };
        let Ok(role_c) = std::ffi::CString::new(relay.role.as_str()) else {
            continue;
        };
        nmp_app_add_relay(s.app, url_c.as_ptr(), role_c.as_ptr());
    }
    nmp_app_start(s.app, visible_limit as u32, emit_hz as u32);
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeStop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_ref(handle) {
        nmp_app_stop(s.app);
    }
}

/// ADR-0063 (#1671) — typed profile-ref resolution for visible gallery authors.
/// Supersedes the deleted `nativeClaimProfile` and avoids exposing raw
/// namespace/shape/liveness discriminants to Kotlin.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeResolveProfileRef(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    key: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(key) = jstring_to_cstring(&mut env, &key) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    nmp_app_resolve_profile_ref(s.app, key.as_ptr(), consumer_id.as_ptr());
}

/// ADR-0063 (#1671) — release a profile ref previously registered via
/// `nativeResolveProfileRef`. Pass the SAME `(key, consumer_id)`.
/// D6: bad handles/strings/unknown int codes are silent no-ops.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeReleaseProfileRef(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    key: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(key) = jstring_to_cstring(&mut env, &key) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    nmp_app_release_profile_ref(s.app, key.as_ptr(), consumer_id.as_ptr());
}

/// Register (or clear) the JNI push listener for kernel update frames
/// (issue #614 — D8 no-polling; replaces the deleted `nativeNextUpdate`
/// blocking drain). Mirrors the Chirp `nativeSetUpdateListener`.
///
/// `listener` must implement `fun onUpdate(frame: ByteArray)`. Frames are
/// pushed from the kernel's update-listener thread; pass `null` to deregister.
/// D6: a null/dead handle or any JNI failure is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeSetUpdateListener(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    listener: JObject,
) {
    let Some(s) = session_ref(handle) else { return };
    set_update_listener(env, s, listener);
}

/// Clear the JNI push listener without freeing the session (issue #614).
/// D6: a null/dead handle is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeClearUpdateListener(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_ref(handle) {
        unsafe { &*s.update_ctx }.clear_listener();
    }
}

/// Dispatch a write through the typed BYTE doorway (ADR-0064 / Cut-B, #1756).
///
/// The Kotlin shell hands us `(action, payload)` — the action's host namespace
/// and the canonical serde body for that write. The
/// [`dispatch_action_bytes_for`] seam deserializes the body into the
/// namespace's typed [`ActionPayload`](nmp_core::substrate::ActionPayload),
/// wraps it in an open dispatch envelope, and hands TYPED BYTES to
/// [`nmp_ffi::nmp_app_dispatch_action_bytes`]. No JSON crosses the FFI; the
/// JSON is an in-process intermediate only.
///
/// Returns the kernel's result envelope JSON (`{"correlation_id":…}` on accept,
/// `{"error":…}` on synchronous rejection). A seam-side failure (null app,
/// unknown / mis-shaped namespace) is surfaced to the UI as a fail-closed
/// `{"error":…}` envelope rather than a silent null, so the unknown-namespace
/// case is observable (D6). A null/dead handle (no live session) is still a
/// null no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeDispatchAction<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    action: JString<'l>,
    payload: JString<'l>,
) -> jstring {
    let null = std::ptr::null_mut();
    let Some(s) = session_ref(handle) else {
        return null;
    };
    let Some(action_c) = jstring_to_cstring(&mut env, &action) else {
        return null;
    };
    let Some(payload_c) = jstring_to_cstring(&mut env, &payload) else {
        return null;
    };
    let (Ok(namespace), Ok(body)) = (action_c.to_str(), payload_c.to_str()) else {
        return null;
    };
    let result = match dispatch_action_bytes_for(s.app, namespace, body) {
        Ok(envelope_json) => envelope_json,
        Err(message) => serde_json::json!({ "error": message }).to_string(),
    };
    match env.new_string(result) {
        Ok(js) => js.into_raw(),
        Err(_) => null,
    }
}

// ── NIP-55 external signer (ADR-0048 Stage 2 / issue #1612) ──────────────

/// Register (or clear) the JNI push listener for outbound NIP-55
/// external-signer requests (issue #1612 — D8 no-polling; replaces the
/// deleted `nativeNextSignerRequest` blocking/timeout drain).
///
/// `listener` must implement `fun onSignerRequest(requestJson: String)`. Each
/// request is pushed from whichever Rust thread dispatches the
/// `external_signer` capability (a background thread), so Kotlin must marshal
/// to the main thread itself before launching a NIP-55 Intent. Pass `null` to
/// deregister.
///
/// D6: a null/dead handle, or any JNI failure obtaining the `JavaVM` / global
/// ref, is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeSetSignerRequestListener(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    listener: JObject,
) {
    let Some(s) = session_ref(handle) else { return };
    if listener.is_null() {
        if let Ok(mut slot) = s.signer_listener.lock() {
            slot.take();
        }
        return;
    }
    let Ok(vm) = env.get_java_vm() else { return };
    let Ok(global) = env.new_global_ref(&listener) else {
        return;
    };
    if let Ok(mut slot) = s.signer_listener.lock() {
        *slot = Some(Arc::new(SignerRequestPushListener::new(vm, global)));
    }
}

/// Clear the JNI signer-request push listener without freeing the session
/// (issue #1612). D6: a null/dead handle is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeClearSignerRequestListener(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_ref(handle) {
        if let Ok(mut slot) = s.signer_listener.lock() {
            slot.take();
        }
    }
}

/// Begin a NIP-55 sign-in. `signer_package` may be null ("let the OS
/// resolver pick"); Rust builds the `get_public_key` + permission-batch
/// request (D7 — Kotlin reports user intent only).
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeSignInNip55(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    signer_package: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let package = {
        let obj: &jni::objects::JObject = AsRef::<jni::objects::JObject>::as_ref(&signer_package);
        if obj.as_raw().is_null() {
            None
        } else {
            jstring_to_cstring(&mut env, &signer_package)
        }
    };
    nmp_app_signin_nip55(
        s.app,
        package.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
    );
}

/// Report a raw `ExternalSignerResponse` JSON back to the Rust driver
/// (D7 — verbatim; the driver owns correlation routing and all policy).
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeDeliverSignerResponse(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    response_json: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(response) = jstring_to_cstring(&mut env, &response_json) else {
        return;
    };
    nmp_app_deliver_external_signer_response(s.app, response.as_ptr());
}
