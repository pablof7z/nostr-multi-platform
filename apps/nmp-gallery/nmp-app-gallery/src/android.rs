//! JNI shim: Android ⇄ the nmp-core kernel for the NmpGallery app.
//!
//! Exports `Java_org_nmp_gallery_bridge_KernelBridge_*` symbols matching the
//! `KernelBridge.kt` `external fun` declarations. Pattern mirrors
//! `apps/chirp/nmp-chirp-android-ffi` which does the same for the Chirp app.
//!
//! Doctrine: no business logic or cached state (D5/D8) — pure transport.
//! Errors never cross FFI (D6); outcomes arrive in the next FlatBuffers
//! snapshot frame.

use std::ffi::{c_void, CStr, CString};
use std::sync::Arc;

use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;

use std::sync::Mutex;

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_claim_event, nmp_app_deliver_external_signer_response,
    nmp_app_dispatch_action, nmp_app_free, nmp_app_new, nmp_app_release_event,
    nmp_app_release_ref, nmp_app_resolve_ref, nmp_app_set_capability_callback,
    nmp_app_set_update_callback, nmp_app_signin_nip55, nmp_app_start, nmp_app_stop,
    nmp_external_signer_init, nmp_free_string, NmpApp,
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
    update_ctx: *mut GalleryUpdateCtx,
    /// Issue #1612 / ADR-0048 Stage 2 — push listener slot for outbound NIP-55
    /// `ExternalSignerRequest` JSON payloads (D8 no-polling; replaces the
    /// deleted `nativeNextSignerRequest` blocking/timeout drain).
    signer_listener: SignerRequestListenerSlot,
    /// ADR-0063 (#1671) — host-side mirror of the kernel's `refs.profile`
    /// row-delta projection. Merged across frames in `nativeDecodeSnapshotJson`
    /// (the sidecar carries only changed/cleared rows). The sole app-side profile
    /// store (D4). Wrapped in a `Mutex` because the decode runs on whichever
    /// thread Kotlin drains the push frame on.
    ref_profiles: Mutex<RefProfileStore>,
}

// SAFETY: GallerySession is transferred to Kotlin as a jlong handle; access
// is serialised by the Kotlin caller (nativeNew → nativeFree lifecycle).
unsafe impl Send for GallerySession {}

/// Update callback — runs on the kernel's listener thread. Forwards the
/// borrowed FlatBuffers frame straight to the registered JNI push listener
/// (issue #614 — D8 no-polling).
extern "C" fn on_update(context: *mut c_void, bytes: *const u8, len: usize) {
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
extern "C" fn on_capability_request(
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

fn to_c_string(value: String) -> *mut std::os::raw::c_char {
    CString::new(value)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

fn session_ref<'a>(handle: jlong) -> Option<&'a GallerySession> {
    if handle == 0 {
        None
    } else {
        Some(unsafe { &*(handle as *const GallerySession) })
    }
}

fn jstring_to_cstring(env: &mut JNIEnv, value: &JString) -> Option<CString> {
    let s = env.get_string(value).ok()?;
    CString::new(s.to_string_lossy().into_owned()).ok()
}

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
    let update_ctx = Box::into_raw(Box::new(GalleryUpdateCtx::new()));
    nmp_app_set_update_callback(app, update_ctx as *mut c_void, Some(on_update));
    // Issue #1612 / ADR-0048 Stage 2 — push-based signer-request delivery
    // (D8 no-polling; replaces the deleted mpsc-channel + nativeNextSignerRequest drain).
    // The trampoline context is a raw pointer to the Mutex inside the Arc.
    let signer_listener: SignerRequestListenerSlot = Arc::new(std::sync::Mutex::new(None));
    // SAFETY: the raw pointer into the Arc-owned Mutex outlives any trampoline
    // call because nativeFree calls `nmp_app_set_capability_callback(…, None)`
    // (which blocks until any in-flight call returns) before the session Arc
    // (and thus the Mutex) is dropped. Dereferencing this pointer in
    // `on_capability_request` is safe for the full session lifetime.
    let trampoline_ctx =
        Arc::as_ptr(&signer_listener) as *mut std::sync::Mutex<Option<Arc<SignerRequestPushListener>>>;
    nmp_app_set_capability_callback(app, trampoline_ctx as *mut c_void, Some(on_capability_request));
    nmp_external_signer_init(app);
    let session = Box::new(GallerySession {
        app,
        update_ctx,
        signer_listener,
        ref_profiles: Mutex::new(RefProfileStore::new()),
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
    unsafe {
        // Quiescence gate: both `set_*_callback(None)` calls block until any
        // in-flight trampoline returns, so the context pointers and the
        // `GlobalRef` inside the signer listener can be dropped safely without
        // a UAF.
        nmp_app_set_update_callback(s.app, std::ptr::null_mut(), None);
        nmp_app_set_capability_callback(s.app, std::ptr::null_mut(), None);
        nmp_app_free(s.app);
        drop(Box::from_raw(s.update_ctx));
        // signer_listener Arc drops here, taking the listener GlobalRef with it.
    }
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
    // ADR-0063 (#1671): merge the frame's `refs.profile` row-delta batch into the
    // session's persistent store before building the snapshot JSON. The handle is
    // optional only for the (pre-session) error path; without it the row-deltas
    // cannot accumulate, so a missing handle yields null (D6).
    let Some(s) = session_ref(handle) else { return null };
    let Ok(mut store) = s.ref_profiles.lock() else {
        return null;
    };
    let Ok(json) = crate::snapshot_json::snapshot_json_from_update_frame(&bytes, &mut store) else {
        return null;
    };
    drop(store);
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
        let Ok(url_c) = CString::new(relay.url.as_str()) else { continue };
        let Ok(role_c) = CString::new(relay.role.as_str()) else { continue };
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
    if let Some(s) = session_ref(handle) { nmp_app_stop(s.app); }
}

/// ADR-0063 (#1671) — unified, origin-blind reference resolution. Supersedes
/// the deleted `nativeClaimProfile`.
///
/// `namespace` — 0 = profile. `key` — lowercase 64-hex pubkey.
/// `consumer_id` — opaque refcount owner key. `shape` — 0 = profile.ref (avatar),
/// 1 = profile.card. `liveness` — 0 = CacheOk (background), non-zero = Live.
/// D6: bad handles/strings/unknown int codes are silent no-ops.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeResolveRef(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    namespace: jint,
    key: JString,
    consumer_id: JString,
    shape: jint,
    liveness: jint,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(key) = jstring_to_cstring(&mut env, &key) else { return };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else { return };
    nmp_app_resolve_ref(
        s.app,
        namespace,
        key.as_ptr(),
        consumer_id.as_ptr(),
        shape,
        liveness,
    );
}

/// ADR-0063 (#1671) — release a reference previously registered via
/// `nativeResolveRef`. Pass the SAME `(namespace, key, consumer_id)`.
/// D6: bad handles/strings/unknown int codes are silent no-ops.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeReleaseRef(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    namespace: jint,
    key: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(key) = jstring_to_cstring(&mut env, &key) else { return };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else { return };
    nmp_app_release_ref(s.app, namespace, key.as_ptr(), consumer_id.as_ptr());
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeClaimEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(uri) = jstring_to_cstring(&mut env, &uri) else { return };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else { return };
    // F-TTL — Android JNI claim is a background/auto-claim → force = 0.
    nmp_app_claim_event(s.app, uri.as_ptr(), consumer_id.as_ptr(), 0);
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeReleaseEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(uri) = jstring_to_cstring(&mut env, &uri) else { return };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else { return };
    nmp_app_release_event(s.app, uri.as_ptr(), consumer_id.as_ptr());
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
    let ctx = unsafe { &*s.update_ctx };
    if listener.is_null() {
        ctx.clear_listener();
        return;
    }
    let Ok(vm) = env.get_java_vm() else { return };
    let Ok(global) = env.new_global_ref(&listener) else { return };
    ctx.set_listener(GalleryUpdateListener::new(vm, global));
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

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeDispatchAction<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    action: JString<'l>,
    payload: JString<'l>,
) -> jstring {
    let null = std::ptr::null_mut();
    let Some(s) = session_ref(handle) else { return null };
    let Some(action_c) = jstring_to_cstring(&mut env, &action) else { return null };
    let Some(payload_c) = jstring_to_cstring(&mut env, &payload) else { return null };
    let ptr = nmp_app_dispatch_action(s.app, action_c.as_ptr(), payload_c.as_ptr());
    if ptr.is_null() {
        return null;
    }
    let result = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    nmp_free_string(ptr);
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
        if let Ok(mut slot) = s.signer_listener.lock() { slot.take(); }
        return;
    }
    let Ok(vm) = env.get_java_vm() else { return };
    let Ok(global) = env.new_global_ref(&listener) else { return };
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
        if let Ok(mut slot) = s.signer_listener.lock() { slot.take(); }
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
        if obj.as_raw().is_null() { None } else { jstring_to_cstring(&mut env, &signer_package) }
    };
    nmp_app_signin_nip55(s.app, package.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()));
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
    let Some(response) = jstring_to_cstring(&mut env, &response_json) else { return };
    nmp_app_deliver_external_signer_response(s.app, response.as_ptr());
}
