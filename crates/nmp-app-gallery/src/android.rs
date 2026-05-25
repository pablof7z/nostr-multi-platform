//! JNI shim: Android ⇄ the nmp-core kernel for the NmpGallery app.
//!
//! Exports `Java_org_nmp_gallery_bridge_KernelBridge_*` symbols matching the
//! `KernelBridge.kt` `external fun` declarations. Pattern mirrors
//! `crates/nmp-android-ffi` which does the same for the Chirp app.
//!
//! Doctrine: no business logic or cached state (D5/D8) — pure transport.
//! Errors never cross FFI (D6); outcomes arrive in the next JSON snapshot.

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_claim_profile, nmp_app_dispatch_action, nmp_app_free,
    nmp_app_free_string, nmp_app_new, nmp_app_open_author, nmp_app_release_profile,
    nmp_app_set_update_callback, nmp_app_start, nmp_app_stop, NmpApp,
};

/// Bootstrap relays — mirrors the iOS `GalleryModel.bootstrapRelays` list.
const BOOTSTRAP_RELAYS: &[&str] = &[
    "wss://purplepag.es",
    "wss://relay.damus.io",
    "wss://nos.lol",
];

/// Owns the kernel handle, snapshot receiver, and boxed sender held by the
/// kernel callback. Freed exactly once in `nativeFree`.
pub(crate) struct GallerySession {
    pub(crate) app: *mut NmpApp,
    rx: Receiver<String>,
    tx: *mut Sender<String>,
}

// SAFETY: GallerySession is transferred to Kotlin as a jlong handle; access
// is serialised by the Kotlin caller (nativeNew → nativeFree lifecycle;
// nativeNextUpdate on one reader thread).
unsafe impl Send for GallerySession {}

/// Callback — runs on the kernel's listener thread. Copies the borrowed JSON
/// into an owned `String` before handing it to the channel (§3 contract).
extern "C" fn on_update(context: *mut c_void, json: *const c_char) {
    if context.is_null() || json.is_null() {
        return;
    }
    let tx = unsafe { &*(context as *const Sender<String>) };
    let owned = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
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
    let (tx, rx) = std::sync::mpsc::channel::<String>();
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
    for url in BOOTSTRAP_RELAYS {
        let Ok(url_c) = CString::new(*url) else {
            continue;
        };
        let Ok(role_c) = CString::new("both") else {
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
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeNextUpdate<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    timeout_ms: jlong,
) -> jstring {
    let null = std::ptr::null_mut();
    let Some(s) = session_ref(handle) else {
        return null;
    };
    let timeout = Duration::from_millis(timeout_ms.max(0) as u64);
    match s.rx.recv_timeout(timeout) {
        Ok(json) => match env.new_string(json) {
            Ok(js) => js.into_raw(),
            Err(_) => null,
        },
        Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => null,
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
