//! JNI shim: Android ⇄ the nmp-core kernel via Rust-path function calls.
//!
//! Doctrine: no business logic or cached state here (D5/D8) — pure transport.
//! Errors never cross FFI (D6): the kernel reports via the JSON snapshot; these
//! entrypoints return only a handle / a string / void. The kernel's update
//! callback fires on its own listener thread with a pointer valid ONLY for the
//! call's duration (`docs/ffi-surface.md` §3), so we copy it into an owned
//! `String` before handing it to a channel. A Kotlin thread drains the channel
//! via `nativeNextUpdate` (blocking, timed) — this sidesteps JNI
//! thread-attach/global-ref complexity while staying a faithful mirror of the
//! iOS push model.
//!
//! WHY Rust paths, not `extern "C"`:
//! `extern "C" { fn nmp_app_new() }` is opaque to Rust CGU compilation — the
//! rlib is consumed at compile time into CGU object files, but only code
//! reachable through RUST paths enters those files. Symbols declared only via
//! `extern "C"` stay `U` (undefined) in the final cdylib. Calling through
//! `nmp_ffi::nmp_app_new()` (enabled by the `android-ffi` feature) is the
//! portable fix that makes rustc include the bodies.

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;

use nmp_app_chirp::{
    nmp_app_chirp_register, nmp_app_chirp_snapshot, nmp_app_chirp_snapshot_free,
    nmp_app_chirp_unregister, ChirpHandle,
};
use nmp_ffi::{
    nmp_app_add_relay, nmp_app_create_new_account, nmp_app_free, nmp_app_new,
    nmp_app_open_timeline, nmp_app_set_update_callback, nmp_app_start, nmp_app_stop, NmpApp,
};

/// Owns the kernel handle, the snapshot receiver, and the boxed sender that the
/// kernel holds as an opaque callback context. Freed exactly once in
/// `nativeFree` (mirrors Swift `KernelHandle.deinit`).
pub(crate) struct Session {
    pub(crate) app: *mut NmpApp,
    chirp: *mut ChirpHandle,
    rx: Receiver<String>,
    tx: *mut Sender<String>,
}

// SAFETY: Session is sent across threads only inside a Box whose ownership is
// transferred to Kotlin as a jlong handle. Access is serialized by the Kotlin
// caller (nativeNew → nativeFree lifecycle; nativeNextUpdate on one reader thread).
unsafe impl Send for Session {}

/// Update callback — runs on the kernel's listener thread. `context` is the
/// `*mut Sender<String>` we registered; `json` is borrowed for this call only.
extern "C" fn on_update(context: *mut c_void, json: *const c_char) {
    if context.is_null() || json.is_null() {
        return;
    }
    // SAFETY: `context` is the pointer passed to `nmp_app_set_update_callback`,
    // alive until `nativeFree` clears the callback before reclaiming it.
    let tx = unsafe { &*(context as *const Sender<String>) };
    // Copy out of the borrowed buffer before it is invalidated (§3).
    let owned = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    let _ = tx.send(owned); // dead receiver ⇒ silent no-op (D6)
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeNew(
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
    let chirp = nmp_app_chirp_register(app, std::ptr::null());
    let session = Box::new(Session { app, chirp, rx, tx });
    Box::into_raw(session) as jlong
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeStart(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    visible_limit: jint,
    emit_hz: jint,
) {
    if let Some(s) = session_ref(handle) {
        nmp_app_start(s.app, 0, visible_limit as u32, emit_hz as u32);
        seed_chirp_reference_relays(s.app);
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeOpenTimeline(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_ref(handle) {
        nmp_app_open_timeline(s.app);
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCreateLocalAccount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    display_name: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let name = env
        .get_string(&display_name)
        .map(|s| s.to_string_lossy().into_owned())
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Android User".to_string());
    let profile = serde_json::json!({ "name": name }).to_string();
    let relays = default_chirp_relays_json();
    let Ok(profile) = CString::new(profile) else {
        return;
    };
    let Ok(relays) = CString::new(relays) else {
        return;
    };
    nmp_app_create_new_account(s.app, profile.as_ptr(), relays.as_ptr(), false);
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeStop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_ref(handle) {
        nmp_app_stop(s.app);
    }
}

/// Blocking drain with a 250 ms timeout so the Kotlin reader thread stays
/// responsive to cancellation. Returns `null` on timeout / closed channel.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeNextUpdate<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) -> jstring {
    let null = std::ptr::null_mut();
    let Some(s) = session_ref(handle) else {
        return null;
    };
    match s.rx.recv_timeout(Duration::from_millis(250)) {
        Ok(json) => match env.new_string(json) {
            Ok(js) => js.into_raw(),
            Err(_) => null,
        },
        Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => null,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeChirpSnapshot<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) -> jstring {
    let null = std::ptr::null_mut();
    let Some(s) = session_ref(handle) else {
        return null;
    };
    if s.chirp.is_null() {
        return null;
    }
    let ptr = nmp_app_chirp_snapshot(s.chirp);
    if ptr.is_null() {
        return null;
    }
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_app_chirp_snapshot_free(ptr);
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => null,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeFree(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    // SAFETY: `handle` was produced by `nativeNew`; freed exactly once.
    let s = unsafe { Box::from_raw(handle as *mut Session) };
    unsafe {
        if !s.chirp.is_null() {
            nmp_app_chirp_unregister(s.chirp);
        }
        nmp_app_set_update_callback(s.app, std::ptr::null_mut(), None);
        nmp_app_free(s.app);
        drop(Box::from_raw(s.tx)); // reclaim the boxed Sender (callback cleared)
    }
}

#[must_use]
pub(crate) fn session_ref<'a>(handle: jlong) -> Option<&'a Session> {
    if handle == 0 {
        None
    } else {
        // SAFETY: non-zero handles are live `Session` pointers from nativeNew
        // until nativeFree; Kotlin never calls after free.
        Some(unsafe { &*(handle as *const Session) })
    }
}

fn seed_chirp_reference_relays(app: *mut NmpApp) {
    for entry in nmp_chirp_config::chirp_default_relay_bootstrap() {
        let Ok(url) = CString::new(entry.url) else {
            continue;
        };
        let Ok(role) = CString::new(entry.role) else {
            continue;
        };
        nmp_app_add_relay(app, url.as_ptr(), role.as_ptr());
    }
}

fn default_chirp_relays_json() -> String {
    let relays = nmp_chirp_config::chirp_default_relay_bootstrap()
        .iter()
        .map(|entry| serde_json::json!([entry.url, entry.role]))
        .collect::<Vec<_>>();
    serde_json::Value::Array(relays).to_string()
}
