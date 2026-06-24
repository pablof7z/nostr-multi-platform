//! JNI shim: Android ⇄ the nmp-core kernel for the NmpGallery app.
//!
//! Exports `Java_org_nmp_gallery_bridge_KernelBridge_*` symbols matching the
//! `KernelBridge.kt` `external fun` declarations. Pattern mirrors
//! `apps/chirp/nmp-chirp-android-ffi` which does the same for the Chirp app.
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

// App-local event URI JNI adapters decode nostr: URIs and route to
// nmp_app_resolve_ref(namespace=1/event) / nmp_app_release_ref.
use nmp_ffi::{
    nmp_app_add_relay, nmp_app_deliver_external_signer_response,
    nmp_app_new, nmp_app_release_ref, nmp_app_resolve_ref,
    nmp_app_set_capability_callback, nmp_app_set_update_callback,
    nmp_app_signin_nip55, nmp_app_start, nmp_app_stop,
    nmp_external_signer_init, nmp_free_string, nmp_nip21_decode_uri,
};

use crate::dispatch_bytes::dispatch_action_bytes_for;

use crate::android_push::{SignerRequestListenerSlot, SignerRequestPushListener};

use nmp_core::refs::RefProfileStore;

use std::sync::Mutex;

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
        let Ok(url_c) = std::ffi::CString::new(relay.url.as_str()) else { continue };
        let Ok(role_c) = std::ffi::CString::new(relay.role.as_str()) else { continue };
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

/// Decode a `nostr:` URI via `nmp_nip21_decode_uri` and return the canonical
/// event key the kernel resolver expects:
///   - nevent / note  → the hex event_id
///   - naddr          → the canonical coordinate string "kind:pubkey:identifier"
/// Returns `None` on any failure (non-event URI, decode error) so callers
/// silently no-op (D6).
fn event_key_from_uri(uri: &std::ffi::CStr) -> Option<std::ffi::CString> {
    // SAFETY: nmp_nip21_decode_uri returns a heap-allocated NUL-terminated
    // JSON string (or a well-formed error JSON).
    let raw = unsafe { nmp_nip21_decode_uri(uri.as_ptr()) };
    if raw.is_null() {
        return None;
    }
    // SAFETY: non-null raw is a valid CStr from nmp_nip21_decode_uri.
    let s = unsafe { std::ffi::CStr::from_ptr(raw) }
        .to_str()
        .ok()
        .map(str::to_owned);
    // SAFETY: raw is freed exactly once here.
    unsafe { nmp_free_string(raw) };
    let s = s?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    if !v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false) {
        return None;
    }
    let key = match v.get("target").and_then(|t| t.as_str()) {
        Some("event") => v.get("event_id").and_then(|e| e.as_str())?.to_owned(),
        Some("address") => {
            let kind = v.get("kind").and_then(|k| k.as_u64())?;
            let pubkey = v.get("pubkey").and_then(|p| p.as_str())?;
            let identifier = v.get("identifier").and_then(|i| i.as_str())?;
            format!("{kind}:{pubkey}:{identifier}")
        }
        _ => return None,
    };
    std::ffi::CString::new(key).ok()
}

/// App-local JNI URI adapter. Decodes the `nostr:` URI in Rust and forwards to
/// `nmp_app_resolve_ref(namespace=1/event, shape=2/embed, liveness=0/CacheOk)`.
/// D6: bad handles / non-event URIs / decode errors are silent no-ops.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeClaimEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(uri_cstr) = jstring_to_cstring(&mut env, &uri) else { return };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else { return };
    let Some(event_key) = event_key_from_uri(&uri_cstr) else { return };
    nmp_app_resolve_ref(
        s.app,
        1, // namespace = event
        event_key.as_ptr(),
        consumer_id.as_ptr(),
        2, // shape = event.embed
        0, // liveness = CacheOk (background/auto-claim)
    );
}

/// App-local JNI URI adapter. Decodes the `nostr:` URI in Rust and forwards to
/// `nmp_app_release_ref(namespace=1/event)`.
/// D6: bad handles / non-event URIs / decode errors are silent no-ops.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeReleaseEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(uri_cstr) = jstring_to_cstring(&mut env, &uri) else { return };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else { return };
    let Some(event_key) = event_key_from_uri(&uri_cstr) else { return };
    nmp_app_release_ref(s.app, 1 /*event*/, event_key.as_ptr(), consumer_id.as_ptr());
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
    let Some(s) = session_ref(handle) else { return null };
    let Some(action_c) = jstring_to_cstring(&mut env, &action) else { return null };
    let Some(payload_c) = jstring_to_cstring(&mut env, &payload) else { return null };
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
