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

use jni::objects::{JClass, JString};
use jni::sys::{jbyteArray, jint, jlong, jstring};
use jni::JNIEnv;

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_claim_event, nmp_app_claim_profile, nmp_app_dispatch_action,
    nmp_app_free, nmp_app_free_string, nmp_app_new, nmp_app_open_author, nmp_app_release_event,
    nmp_app_release_profile, nmp_app_set_update_callback, nmp_app_start, nmp_app_stop, NmpApp,
};

/// Owns the kernel handle, snapshot receiver, and boxed sender held by the
/// kernel callback. Freed exactly once in `nativeFree`.
pub(crate) struct GallerySession {
    pub(crate) app: *mut NmpApp,
    rx: Receiver<Vec<u8>>,
    tx: *mut Sender<Vec<u8>>,
}

// SAFETY: GallerySession is transferred to Kotlin as a jlong handle; access
// is serialised by the Kotlin caller (nativeNew → nativeFree lifecycle;
// nativeNextUpdate on one reader thread).
unsafe impl Send for GallerySession {}

/// Callback — runs on the kernel's listener thread. Copies the borrowed
/// FlatBuffers frame before handing it to the channel.
extern "C" fn on_update(context: *mut c_void, bytes: *const u8, len: usize) {
    if context.is_null() || bytes.is_null() {
        return;
    }
    let tx = unsafe { &*(context as *const Sender<Vec<u8>>) };
    let owned = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
    let _ = tx.send(owned);
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
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let tx = Box::into_raw(Box::new(tx));
    nmp_app_set_update_callback(app, tx as *mut c_void, Some(on_update));
    let session = Box::new(GallerySession { app, rx, tx });
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
        nmp_app_set_update_callback(s.app, std::ptr::null_mut(), None);
        nmp_app_free(s.app);
        drop(Box::from_raw(s.tx));
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

/// Open the author view for `pubkey`. Triggers a kind:0 + kind:10002 fetch
/// and populates `projections.author_view` on every subsequent snapshot tick.
/// Mirrors `nmp_app_open_author` from the iOS shell.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeOpenAuthor(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    nmp_app_open_author(s.app, pubkey.as_ptr());
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
    nmp_app_claim_profile(s.app, pubkey.as_ptr(), consumer_id.as_ptr());
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
    nmp_app_claim_event(s.app, uri.as_ptr(), consumer_id.as_ptr());
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

/// Drain one FlatBuffers update frame from the kernel callback channel.
///
/// V-57 P5: the two `recv_timeout` error arms have distinct meanings and must
/// not be conflated — earlier revisions returned `null` for both, which made
/// the Kotlin polling coroutine spin (the `?: continue` arm in
/// `GalleryModel.startPolling`) once the channel had closed.
///
/// * [`RecvTimeoutError::Timeout`] — normal idle tick. Return `null`; the
///   Kotlin caller loops back into `nextUpdate`. This is the steady state
///   between snapshot emits at `emit_hz`.
/// * [`RecvTimeoutError::Disconnected`] — the boxed [`Sender`] inside this
///   [`GallerySession`] has been dropped. The only drop site is
///   [`Java_org_nmp_gallery_bridge_KernelBridge_nativeFree`], which calls
///   `nmp_app_free` (joining the actor thread) before dropping the boxed
///   sender. The Kotlin coroutine MUST stop polling — we surface this as an
///   `IllegalStateException` so the `viewModelScope` reader breaks out of its
///   `while (isActive)` loop instead of busy-spinning on a dead channel.
///
/// Reachability note: in the current architecture, `Disconnected` is only
/// observable in the narrow window between sender-drop and session-drop
/// inside `nativeFree`. This fix is defensive hardening matching the V-57 P5
/// BACKLOG entry's intent, not a reproduced spin from production logs.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeNextUpdate<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    timeout_ms: jlong,
) -> jbyteArray {
    let null = std::ptr::null_mut();
    let Some(s) = session_ref(handle) else {
        return null;
    };
    let timeout = Duration::from_millis(timeout_ms.max(0) as u64);
    match s.rx.recv_timeout(timeout) {
        Ok(bytes) => match env.byte_array_from_slice(&bytes) {
            Ok(array) => array.into_raw(),
            Err(_) => null,
        },
        // Normal idle tick — Kotlin caller loops back into `nextUpdate`.
        Err(RecvTimeoutError::Timeout) => null,
        // Sender dropped: signal Kotlin to stop polling by raising a JNI
        // exception. Per the JNI contract, no further env calls are issued
        // after `throw_new`; we return null which the JVM ignores in favour
        // of the pending exception on the Rust → Java return.
        Err(RecvTimeoutError::Disconnected) => {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                "gallery snapshot channel disconnected",
            );
            null
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeGallerySnapshot<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) -> jstring {
    let null = std::ptr::null_mut();
    let Some(s) = session_ref(handle) else {
        return null;
    };
    let ptr = crate::nmp_app_gallery_snapshot(s.app as *mut c_void);
    if ptr.is_null() {
        return null;
    }
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    crate::nmp_app_gallery_snapshot_free(ptr);
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => null,
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
    nmp_app_free_string(ptr);
    match env.new_string(result) {
        Ok(js) => js.into_raw(),
        Err(_) => null,
    }
}
