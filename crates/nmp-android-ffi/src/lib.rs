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

use std::ffi::CString;
use std::sync::Arc;
use std::time::Duration;

use jni::objects::{JClass, JString};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;

use nmp_app_chirp::nmp_app_chirp_register;

// Marmot (MLS-over-Nostr) JNI entry points + symbol-retention glue live in their
// own module to keep this transport file cohesive (and off the file-size
// ceiling). The `#[no_mangle] Java_…` symbol names are unaffected by module
// nesting.
mod marmot;
mod session;
use nmp_ffi::{
    nmp_app_add_relay, nmp_app_claim_profile, nmp_app_create_new_account, nmp_app_dispatch_action,
    nmp_app_free_string, nmp_app_new, nmp_app_open_author, nmp_app_open_thread,
    nmp_app_open_timeline, nmp_app_release_profile, nmp_app_remove_account, nmp_app_remove_relay,
    nmp_app_signin_nsec, nmp_app_start, nmp_app_stop, nmp_app_switch_active, NmpApp,
};
use session::{insert_session, remove_session, NextUpdate};
pub(crate) use session::{session_arc, Session};

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeNew(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    let app = nmp_app_new();
    if app.is_null() {
        return 0;
    }
    let chirp = nmp_app_chirp_register(app, std::ptr::null());
    let session = Arc::new(Session::new(app, chirp));
    insert_session(session)
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeStart(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    visible_limit: jint,
    emit_hz: jint,
) {
    if let Some(s) = session_arc(handle) {
        s.with_app(|app| {
            nmp_app_start(app, 0, visible_limit as u32, emit_hz as u32);
            seed_chirp_reference_relays(app);
        });
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeClose(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.close_updates();
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeOpenTimeline(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.with_app(|app| nmp_app_open_timeline(app));
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCreateLocalAccount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    display_name: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let name = env
        .get_string(&display_name)
        .map(|s| s.to_string_lossy().into_owned())
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Android User".to_string());
    // nmp_app_create_new_account expects:
    //   profile_json = {"name":"…"}
    //   relays_json  = [["url","role"],…]  (Vec<(String,String)> serde shape)
    let profile_json = format!(r#"{{"name":"{}"}}"#, name.replace('"', ""));
    let relays_json = default_chirp_relays_json_array();
    let (Ok(profile_c), Ok(relays_c)) = (CString::new(profile_json), CString::new(relays_json))
    else {
        return;
    };
    s.with_app(|app| {
        nmp_app_create_new_account(app, profile_c.as_ptr(), relays_c.as_ptr(), false, 1);
    });
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeStop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.with_app(|app| nmp_app_stop(app));
    }
}

/// Blocking binary drain with a 250 ms timeout so the Kotlin reader thread
/// stays responsive to cancellation.
///
/// Return contract (mirrors PR #644 / V-57 P5 for nmp-gallery — the two
/// `recv_timeout` arms have distinct meanings and must NOT be conflated):
///
/// * timeout — normal idle tick. Returns `null`; the
///   Kotlin caller loops back into `nextUpdate`. This is the steady state
///   between snapshot emits at `emit_hz`.
/// * closed — `nativeClose`/`nativeFree` has closed the callback sender. The
///   reader gets a JNI `java.lang.IllegalStateException` and stops before
///   Kotlin frees the handle id.
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
    let Some(s) = session_arc(handle) else {
        return null;
    };
    match s.recv_next_update(Duration::from_millis(250)) {
        NextUpdate::Frame(bytes) => match env.byte_array_from_slice(&bytes) {
            Ok(array) => array.into_raw(),
            Err(_) => null,
        },
        NextUpdate::Idle => null,
        NextUpdate::Closed => {
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
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    // F-TTL — the Android JNI claim is a background/auto-claim, so force = 0
    // (the lazy, TTL-gated path). User-navigation force-refresh is a Swift-app
    // feature; the Android bridge does not expose a `force` parameter (V-109:
    // Android is largely unwired).
    s.with_app(|app| {
        nmp_app_claim_profile(app, pubkey.as_ptr(), consumer_id.as_ptr(), 0);
    });
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
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    s.with_app(|app| {
        nmp_app_release_profile(app, pubkey.as_ptr(), consumer_id.as_ptr());
    });
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
    let Some(s) = session_arc(handle) else {
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
    let Some(result_ptr) =
        s.with_app(|app| nmp_app_dispatch_action(app, namespace.as_ptr(), action_json.as_ptr()))
    else {
        return env
            .new_string("{}")
            .unwrap_or_else(|_| env.new_string("{}").unwrap())
            .into_raw();
    };
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
    // nmp_app_create_new_account deserialises relays_json as Vec<(String,String)>,
    // so the wire shape must be [["url","role"],…], NOT [{"url":"…","role":"…"},…].
    let relays: Vec<[&str; 2]> = nmp_chirp_config::chirp_default_relay_bootstrap()
        .iter()
        .map(|e| [e.url, e.role])
        .collect();
    serde_json::to_string(&relays).unwrap_or_else(|_| "[]".to_string())
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
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(note_id) = jstring_to_cstring(&mut env, &note_id) else {
        return;
    };
    s.with_app(|app| nmp_app_open_thread(app, note_id.as_ptr()));
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
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    s.with_app(|app| nmp_app_open_author(app, pubkey.as_ptr()));
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
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(url) = jstring_to_cstring(&mut env, &url) else {
        return;
    };
    let Some(role) = jstring_to_cstring(&mut env, &role) else {
        return;
    };
    s.with_app(|app| nmp_app_add_relay(app, url.as_ptr(), role.as_ptr()));
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
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(url) = jstring_to_cstring(&mut env, &url) else {
        return;
    };
    s.with_app(|app| nmp_app_remove_relay(app, url.as_ptr()));
}

/// Sign in with an nsec secret key.
///
/// D6: null handle or invalid secret is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSignInNsec(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    secret: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(secret) = jstring_to_cstring(&mut env, &secret) else {
        return;
    };
    s.with_app(|app| nmp_app_signin_nsec(app, secret.as_ptr(), 1));
}

/// Switch the active account to the given pubkey.
///
/// D6: null handle or invalid pubkey is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSwitchAccount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    s.with_app(|app| nmp_app_switch_active(app, pubkey.as_ptr()));
}

/// Remove an account by pubkey.
///
/// D6: null handle or invalid pubkey is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeRemoveAccount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    s.with_app(|app| nmp_app_remove_account(app, pubkey.as_ptr()));
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeFree(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = remove_session(handle) {
        // `free_native` first closes the update sender, waking any blocked
        // `nativeNextUpdate`. The `Arc<Session>` held by that reader keeps the
        // receiver allocation alive until the JNI call returns.
        s.free_native();
    }
}

/// Copy a Java `JString` into an owned `CString` ready for handing across the
/// C-ABI seam. Returns `None` if the `JString` was null or the JNI fetch
/// failed; `nmp_app_*` shims downstream of this treat `None` as a silent
/// no-op (D6).
pub(crate) fn jstring_to_cstring(env: &mut JNIEnv, value: &JString) -> Option<CString> {
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
