//! JNI shim: Android ⇄ the nmp-core kernel for the NmpGallery app.
//!
//! Exports `Java_org_nmp_gallery_bridge_KernelBridge_*` symbols matching the
//! `KernelBridge.kt` `external fun` declarations. Pattern mirrors
//! `crates/nmp-android-ffi` which does the same for the Chirp app.
//!
//! Doctrine: no business logic or cached state (D5/D8) — pure transport.
//! Errors never cross FFI (D6); outcomes arrive in the next FlatBuffers
//! snapshot frame.

use std::ffi::{c_void, CStr, CString};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_claim_event, nmp_app_claim_profile, nmp_app_encode_profile,
    nmp_app_deliver_external_signer_response, nmp_app_dispatch_action, nmp_app_free, nmp_app_new,
    nmp_app_release_event, nmp_app_release_profile, nmp_app_set_capability_callback,
    nmp_app_set_update_callback, nmp_app_signin_nip55, nmp_app_start, nmp_app_stop,
    nmp_external_signer_init, nmp_free_string, NmpApp,
};

use nmp_core::__ffi_internal::capability_error_envelope;

use crate::android_push::{GalleryUpdateCtx, GalleryUpdateListener};

/// Owns the kernel handle, the boxed update-callback context (which holds the
/// JNI push listener), and the boxed signer-request sender. Freed exactly once
/// in `nativeFree`.
pub(crate) struct GallerySession {
    pub(crate) app: *mut NmpApp,
    /// Boxed [`GalleryUpdateCtx`] passed as the `nmp_app_set_update_callback`
    /// context. Owns the JNI push listener slot (issue #614 — D8 no-polling).
    update_ctx: *mut GalleryUpdateCtx,
    /// ADR-0048 Stage 2 — outbound NIP-55 `ExternalSignerRequest` JSON
    /// payloads, pushed by the capability trampoline and drained by Kotlin
    /// via `nativeNextSignerRequest`.
    signer_rx: Receiver<String>,
    signer_tx: *mut Sender<String>,
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

/// Capability trampoline (ADR-0048 Stage 2). `external_signer` requests are
/// pushed onto the session's signer channel and acked with
/// `{"status":"dispatched"}`; everything else gets the same error envelope a
/// missing handler would (no Android keyring capability exists yet). Context
/// is the boxed signer `Sender` — same ownership pattern as `on_update`.
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
    if parsed.get("namespace").and_then(|v| v.as_str()) != Some("external_signer") {
        return to_c_string(capability_error_envelope(&request, "unsupported-on-android"));
    }
    let Some(payload) = parsed.get("payload_json").and_then(|v| v.as_str()) else {
        return to_c_string(capability_error_envelope(&request, "missing-payload"));
    };
    let tx = unsafe { &*(context as *const Sender<String>) };
    if tx.send(payload.to_string()).is_err() {
        return to_c_string(capability_error_envelope(&request, "session-closed"));
    }
    let envelope = serde_json::json!({
        "namespace": "external_signer",
        "correlation_id": parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
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
    // ADR-0048 Stage 2 — external-signer capability trampoline + NIP-55
    // driver init (gallery hosts the canonical login-block component).
    let (signer_tx, signer_rx) = std::sync::mpsc::channel::<String>();
    let signer_tx = Box::into_raw(Box::new(signer_tx));
    nmp_app_set_capability_callback(app, signer_tx as *mut c_void, Some(on_capability_request));
    nmp_external_signer_init(app);
    let session = Box::new(GallerySession {
        app,
        update_ctx,
        signer_rx,
        signer_tx,
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
        // Quiescence gate: `set_update_callback(None)` blocks until any
        // in-flight `on_update` returns, so the `GalleryUpdateCtx` (and its JNI
        // listener `GlobalRef`) can be dropped below without a UAF (issue #614).
        nmp_app_set_update_callback(s.app, std::ptr::null_mut(), None);
        nmp_app_set_capability_callback(s.app, std::ptr::null_mut(), None);
        nmp_app_free(s.app);
        drop(Box::from_raw(s.update_ctx));
        drop(Box::from_raw(s.signer_tx));
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
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeEncodeProfile<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    pubkey: JString<'l>,
) -> jstring {
    let Some(s) = session_ref(handle) else {
        return std::ptr::null_mut();
    };
    let Some(pubkey_c) = jstring_to_cstring(&mut env, &pubkey) else {
        return std::ptr::null_mut();
    };
    let raw_ptr = nmp_app_encode_profile(s.app, pubkey_c.as_ptr());
    if raw_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let encoded = unsafe { CStr::from_ptr(raw_ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_free_string(raw_ptr);
    env.new_string(encoded)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
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
    let Some(s) = session_ref(handle) else {
        return;
    };
    for relay in &crate::showcase::references().relays {
        let Ok(url_c) = CString::new(relay.url.as_str()) else {
            continue;
        };
        let Ok(role_c) = CString::new(relay.role.as_str()) else {
            continue;
        };
        nmp_app_add_relay(s.app, url_c.as_ptr(), role_c.as_ptr());
    }
    nmp_app_start(s.app, 0, visible_limit as u32, emit_hz as u32);
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

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeClaimProfile(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    // F-TTL — Android JNI claim is a background/auto-claim → force = 0.
    nmp_app_claim_profile(s.app, pubkey.as_ptr(), consumer_id.as_ptr(), 0, 0);
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeReleaseProfile(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    nmp_app_release_profile(s.app, pubkey.as_ptr(), consumer_id.as_ptr());
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeClaimEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(uri) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
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
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(uri) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
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
    let Some(s) = session_ref(handle) else {
        return;
    };
    let ctx = unsafe { &*s.update_ctx };
    if listener.is_null() {
        ctx.clear_listener();
        return;
    }
    let Ok(vm) = env.get_java_vm() else {
        return;
    };
    let Ok(global) = env.new_global_ref(&listener) else {
        return;
    };
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
    let Some(s) = session_ref(handle) else {
        return null;
    };
    let Some(action_c) = jstring_to_cstring(&mut env, &action) else {
        return null;
    };
    let Some(payload_c) = jstring_to_cstring(&mut env, &payload) else {
        return null;
    };
    let ptr = nmp_app_dispatch_action(s.app, action_c.as_ptr(), payload_c.as_ptr());
    if ptr.is_null() {
        return null;
    }
    let result = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_free_string(ptr);
    match env.new_string(result) {
        Ok(js) => js.into_raw(),
        Err(_) => null,
    }
}

// ── NIP-55 external signer (ADR-0048 Stage 2) ─────────────────────────────

/// Blocking timed drain of the outbound NIP-55 request channel. Same return
/// contract as `nativeNextUpdate`: `null` = idle tick, a `String` = one
/// `ExternalSignerRequest` JSON, `IllegalStateException` = channel closed
/// (the Kotlin reader MUST stop).
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeNextSignerRequest<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    timeout_ms: jlong,
) -> jstring {
    let null = std::ptr::null_mut();
    let Some(s) = session_ref(handle) else {
        return null;
    };
    let timeout = Duration::from_millis(timeout_ms.max(0) as u64);
    match s.signer_rx.recv_timeout(timeout) {
        Ok(payload) => match env.new_string(payload) {
            Ok(js) => js.into_raw(),
            Err(_) => null,
        },
        Err(RecvTimeoutError::Timeout) => null,
        Err(RecvTimeoutError::Disconnected) => {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                "gallery signer request channel disconnected",
            );
            null
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
    let Some(s) = session_ref(handle) else {
        return;
    };
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
        package
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr()),
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
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(response) = jstring_to_cstring(&mut env, &response_json) else {
        return;
    };
    nmp_app_deliver_external_signer_response(s.app, response.as_ptr());
}
