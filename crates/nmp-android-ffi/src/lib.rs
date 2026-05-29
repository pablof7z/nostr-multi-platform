//! JNI shim: Android ⇄ the nmp-core kernel via Rust-path function calls.
//!
//! Doctrine: no business logic or cached state here (D5/D8) — pure transport.
//! Errors never cross FFI (D6): the kernel reports via update frames; these
//! entrypoints return only a handle / bytes / void. The kernel's update
//! callback fires on its own listener thread with a pointer valid ONLY for the
//! call's duration (`docs/ffi-surface.md` §3), so we copy it into owned bytes
//! before handing it to a channel. A Kotlin thread drains the channel via
//! `nativeNextUpdate` (blocking, timed) — this sidesteps JNI
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

use std::ffi::{c_void, CString};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use jni::objects::{JClass, JString};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;

use nmp_app_chirp::{nmp_app_chirp_register, nmp_app_chirp_unregister, ChirpHandle};
use nmp_ffi::{
    nmp_app_add_relay, nmp_app_claim_profile, nmp_app_dispatch_action, nmp_app_free,
    nmp_app_free_string, nmp_app_new, nmp_app_open_author, nmp_app_open_thread,
    nmp_app_open_timeline, nmp_app_release_profile, nmp_app_remove_relay,
    nmp_app_set_update_callback, nmp_app_start, nmp_app_stop, NmpApp,
};

/// Owns the kernel handle, the snapshot receiver, and the boxed sender that the
/// kernel holds as an opaque callback context. Freed exactly once in
/// `nativeFree` (mirrors Swift `KernelHandle.deinit`).
pub(crate) struct Session {
    pub(crate) app: *mut NmpApp,
    chirp: *mut ChirpHandle,
    rx: Receiver<Vec<u8>>,
    tx: *mut Sender<Vec<u8>>,
}

// SAFETY: Session is sent across threads only inside a Box whose ownership is
// transferred to Kotlin as a jlong handle. Access is serialized by the Kotlin
// caller (nativeNew → nativeFree lifecycle; nativeNextUpdate on one reader thread).
unsafe impl Send for Session {}

/// Update callback — runs on the kernel's listener thread. `context` is the
/// `*mut Sender<Vec<u8>>` we registered; the FlatBuffers payload is borrowed
/// for this call only.
extern "C" fn on_update(context: *mut c_void, bytes: *const u8, len: usize) {
    if context.is_null() || bytes.is_null() {
        return;
    }
    // SAFETY: `context` is the pointer passed to `nmp_app_set_update_callback`,
    // alive until `nativeFree` clears the callback before reclaiming it.
    let tx = unsafe { &*(context as *const Sender<Vec<u8>>) };
    // Copy out of the borrowed buffer before it is invalidated (§3).
    let owned = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
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
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
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
    let relays_json = default_chirp_relays_json_array();
    let action = format!(
        r#"{{"CreateAccount":{{"profile":{{"name":"{name}"}},"relays":{relays_json},"mls":false}}}}"#,
        name = name.replace('"', ""),
        relays_json = relays_json
    );
    let Ok(ns_c) = CString::new("nmp.create_account") else {
        return;
    };
    let Ok(action_c) = CString::new(action) else {
        return;
    };
    let result_ptr = nmp_app_dispatch_action(s.app, ns_c.as_ptr(), action_c.as_ptr());
    if !result_ptr.is_null() {
        nmp_ffi::nmp_app_free_string(result_ptr);
    }
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

/// Blocking binary drain with a 250 ms timeout so the Kotlin reader thread
/// stays responsive to cancellation.
///
/// Return contract (mirrors PR #644 / V-57 P5 for nmp-gallery — the two
/// `recv_timeout` arms have distinct meanings and must NOT be conflated):
///
/// * [`RecvTimeoutError::Timeout`] — normal idle tick. Returns `null`; the
///   Kotlin caller loops back into `nextUpdate`. This is the steady state
///   between snapshot emits at `emit_hz`.
/// * [`RecvTimeoutError::Disconnected`] — the boxed [`Sender`] inside this
///   [`Session`] has been dropped. The only drop site is
///   [`Java_org_nmp_android_KernelBridge_nativeFree`], which calls
///   `nmp_app_free` (joining the actor thread) before dropping the boxed
///   sender. Surfaces as a JNI `java.lang.IllegalStateException` so the
///   Kotlin reader coroutine breaks out of its `while (isActive)` loop
///   instead of busy-spinning on a dead channel.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeNextUpdateBytes<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) -> jbyteArray {
    next_update_byte_array(env, handle)
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeNextUpdate<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) -> jbyteArray {
    next_update_byte_array(env, handle)
}

fn next_update_byte_array<'l>(mut env: JNIEnv<'l>, handle: jlong) -> jbyteArray {
    let null = std::ptr::null_mut();
    let Some(s) = session_ref(handle) else {
        return null;
    };
    match s.rx.recv_timeout(Duration::from_millis(250)) {
        Ok(bytes) => match env.byte_array_from_slice(&bytes) {
            Ok(array) => array.into_raw(),
            Err(_) => null,
        },
        // Normal idle tick — Kotlin caller loops back into `nextUpdate`.
        Err(RecvTimeoutError::Timeout) => null,
        // Sender dropped: raise a JNI exception so the Kotlin reader breaks
        // out of its polling loop instead of spinning on a dead channel.
        // Per the JNI contract, we do no further env calls after `throw_new`
        // and return null; the JVM honours the pending exception on return.
        Err(RecvTimeoutError::Disconnected) => {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                "kernel update channel closed",
            );
            null
        }
    }
}

/// Demand-driven profile fetch claim: the UI is rendering `pubkey` under
/// `consumer_id`; the kernel batches a kind:0 REQ against the indexer lane
/// (or the author's NIP-65 write set once known). Same contract as the iOS
/// `nmp_app_claim_profile` symbol; calls through to it directly.
///
/// D6 — null/invalid argument is a silent no-op. Non-hex pubkeys are
/// dropped by the underlying `nmp_app_claim_profile` (the kernel's hex
/// gate guards correctness across all FFI surfaces).
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeClaimProfile(
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

/// Demand-driven profile fetch release: the UI no longer needs `pubkey`
/// under `consumer_id`. When the last consumer releases, the kernel
/// reclaims the entry from `profile_claims`. Same contract as the iOS
/// `nmp_app_release_profile` symbol.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeReleaseProfile(
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

/// Dispatch a named action through the action registry.
///
/// Returns a JSON C string the caller receives as a jstring. The caller need not
/// free it — JNI String lifetime is managed by the VM.
///
/// * `{"correlation_id":"<32-hex>"}` — the action was accepted and assigned a
///   correlation id.
/// * `{"error":"<message>"}` — the action was rejected (null app, invalid
///   arguments, unknown namespace, malformed JSON).
///
/// D6: on null handle or any error, returns "{}" (empty JSON object).
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeDispatchAction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    namespace: JString,
    action_json: JString,
) -> jni::sys::jstring {
    let Some(s) = session_ref(handle) else {
        return env
            .new_string("{}")
            .unwrap_or_else(|_| env.new_string("{}").unwrap())
            .into_raw();
    };
    let Some(namespace) = jstring_to_cstring(&mut env, &namespace) else {
        return env
            .new_string("{}")
            .unwrap_or_else(|_| env.new_string("{}").unwrap())
            .into_raw();
    };
    let Some(action_json) = jstring_to_cstring(&mut env, &action_json) else {
        return env
            .new_string("{}")
            .unwrap_or_else(|_| env.new_string("{}").unwrap())
            .into_raw();
    };

    // Call the FFI function; it returns a heap-allocated C string we must free.
    let result_ptr = nmp_app_dispatch_action(s.app, namespace.as_ptr(), action_json.as_ptr());
    if result_ptr.is_null() {
        return env
            .new_string("{}")
            .unwrap_or_else(|_| env.new_string("{}").unwrap())
            .into_raw();
    }

    // Convert to a Rust string, then to JString.
    let result_str = unsafe { std::ffi::CStr::from_ptr(result_ptr) }
        .to_string_lossy()
        .into_owned();

    // Free the C string.
    nmp_app_free_string(result_ptr);

    // Return as jstring.
    env.new_string(&result_str)
        .unwrap_or_else(|_| env.new_string("{}").unwrap())
        .into_raw()
}

fn default_chirp_relays_json_array() -> String {
    let relays: Vec<serde_json::Value> = nmp_chirp_config::chirp_default_relay_bootstrap()
        .iter()
        .map(|e| serde_json::json!({"url": e.url, "role": e.role}))
        .collect();
    serde_json::Value::Array(relays).to_string()
}

/// Open a thread by note ID.
///
/// D6: null handle or invalid note_id is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeOpenThread(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    note_id: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(note_id) = jstring_to_cstring(&mut env, &note_id) else {
        return;
    };
    nmp_app_open_thread(s.app, note_id.as_ptr());
}

/// Open an author by pubkey.
///
/// D6: null handle or invalid pubkey is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeOpenAuthor(
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

/// Add a relay by URL and role string ("read", "write", or "both").
///
/// D6: null handle, null URL, or null role is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeAddRelay(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    url: JString,
    role: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(url) = jstring_to_cstring(&mut env, &url) else {
        return;
    };
    let Some(role) = jstring_to_cstring(&mut env, &role) else {
        return;
    };
    nmp_app_add_relay(s.app, url.as_ptr(), role.as_ptr());
}

/// Remove a relay by URL.
///
/// D6: null handle or null URL is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeRemoveRelay(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    url: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(url) = jstring_to_cstring(&mut env, &url) else {
        return;
    };
    nmp_app_remove_relay(s.app, url.as_ptr());
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

/// Copy a Java `JString` into an owned `CString` ready for handing across the
/// C-ABI seam. Returns `None` if the `JString` was null or the JNI fetch
/// failed; `nmp_app_*` shims downstream of this treat `None` as a silent
/// no-op (D6).
fn jstring_to_cstring(env: &mut JNIEnv, value: &JString) -> Option<CString> {
    let java_str = env.get_string(value).ok()?;
    let owned = java_str.to_string_lossy().into_owned();
    CString::new(owned).ok()
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

