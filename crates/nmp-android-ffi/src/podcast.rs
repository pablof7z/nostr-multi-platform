//! JNI shim: `PodcastKernelBridge` (Kotlin) ⇄ `nmp-app-podcast` (Rust).
//!
//! Mirrors the six `extern "C"` symbols in `nmp-app-podcast::ffi` into JNI-
//! named exports so Kotlin can call them via `external fun`. Called via Rust
//! paths (not `extern "C"` redecl) so the linker includes the symbol bodies
//! in `libnmp_android_ffi.so` (same rationale as `lib.rs` §3 WHY comment).
//!
//! Lifecycle (mirrors iOS pattern):
//!   1. Kotlin calls `nativeRegister(kernelHandle)` — passes the jlong Session
//!      pointer; we extract `session.app` and call `nmp_app_podcast_register`.
//!   2. Kotlin holds the returned jlong (podcast handle) alongside the kernel
//!      Session jlong — two independent lifetimes, just as iOS holds both.
//!   3. `nativeSnapshot` → JSON string (null on failure, D6).
//!   4. `nativeSubscribe` → true on non-null new row, false on dedup/failure.
//!   5. `nativeUnsubscribe` → fire-and-forget (void / Unit).
//!   6. `nativeUnregister` → drops the handle; must precede kernel free.
//!
//! Doctrine:
//!   * D0 — nmp-core gains zero podcast nouns.
//!   * D5 — no business logic or cached state here; pure transport.
//!   * D6 — null handles, null strings, and lock poison all degrade silently.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use nmp_app_podcast::{
    nmp_app_podcast_register, nmp_app_podcast_snapshot, nmp_app_podcast_snapshot_free,
    nmp_app_podcast_subscribe, nmp_app_podcast_unregister, nmp_app_podcast_unsubscribe,
};

use crate::Session;

// ---------------------------------------------------------------------------
// Register / Unregister
// ---------------------------------------------------------------------------

/// `PodcastKernelBridge.nativeRegister(kernelHandle: Long): Long`
///
/// Borrows `kernelHandle` (a live `Session*`) to extract `session.app`, then
/// calls `nmp_app_podcast_register`. Returns the `*mut PodcastHandle` cast to
/// jlong, or 0 on failure (null kernel handle / null session).
#[no_mangle]
pub extern "system" fn Java_com_podcast_app_android_bridge_PodcastKernelBridge_nativeRegister(
    _env: JNIEnv,
    _class: JClass,
    kernel_handle: jlong,
) -> jlong {
    let Some(session) = crate::session_ref(kernel_handle) else {
        return 0;
    };
    let ph = unsafe { nmp_app_podcast_register(session.app) };
    if ph.is_null() { 0 } else { ph as jlong }
}

/// `PodcastKernelBridge.nativeUnregister(podcastHandle: Long)`
#[no_mangle]
pub extern "system" fn Java_com_podcast_app_android_bridge_PodcastKernelBridge_nativeUnregister(
    _env: JNIEnv,
    _class: JClass,
    podcast_handle: jlong,
) {
    if podcast_handle == 0 {
        return;
    }
    unsafe {
        nmp_app_podcast_unregister(podcast_handle as *mut _);
    }
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// `PodcastKernelBridge.nativeSnapshot(podcastHandle: Long): String?`
///
/// Returns a JSON-encoded `LibraryView` (`{"podcasts":[…]}`), or null on any
/// failure (D6). The returned jstring is a fresh JNI local reference; the
/// caller may use it for the duration of the current JNI frame.
#[no_mangle]
pub extern "system" fn Java_com_podcast_app_android_bridge_PodcastKernelBridge_nativeSnapshot<
    'l,
>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    podcast_handle: jlong,
) -> jstring {
    let null = std::ptr::null_mut();
    if podcast_handle == 0 {
        return null;
    }
    let ptr: *mut c_char = unsafe { nmp_app_podcast_snapshot(podcast_handle as *mut _) };
    if ptr.is_null() {
        return null;
    }
    let json_owned = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    unsafe { nmp_app_podcast_snapshot_free(ptr) };
    match env.new_string(&json_owned) {
        Ok(js) => js.into_raw(),
        Err(_) => null,
    }
}

// ---------------------------------------------------------------------------
// Subscribe / Unsubscribe
// ---------------------------------------------------------------------------

/// `PodcastKernelBridge.nativeSubscribe(podcastHandle: Long, feedUrl: String, title: String?, author: String?): Boolean`
///
/// Returns JNI_TRUE if the snapshot changed (new podcast added), JNI_FALSE on
/// dedup or any error. We detect "changed" by comparing snapshot sizes before
/// and after — a simple but reliable proxy for the void FFI return (D6: no
/// error crosses FFI; outcomes surface in the snapshot).
#[no_mangle]
pub extern "system" fn Java_com_podcast_app_android_bridge_PodcastKernelBridge_nativeSubscribe<
    'l,
>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    podcast_handle: jlong,
    feed_url: JString<'l>,
    title: JString<'l>,
    author: JString<'l>,
) -> jboolean {
    if podcast_handle == 0 {
        return JNI_FALSE;
    }
    let ph_ptr: *mut nmp_app_podcast::PodcastHandle = podcast_handle as *mut _;

    // Snapshot count before — we use JSON length as a cheap proxy.
    let before_len = snapshot_json_len(ph_ptr);

    // Convert JStrings → CStrings (null-safe: empty JString → null C ptr).
    let feed_cstr = match jstring_to_cstring(&mut env, feed_url) {
        Some(s) => s,
        None => return JNI_FALSE,
    };
    let title_cstr = jstring_to_cstring(&mut env, title);
    let author_cstr = jstring_to_cstring(&mut env, author);

    let title_ptr = title_cstr.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null());
    let author_ptr = author_cstr.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null());

    unsafe {
        nmp_app_podcast_subscribe(ph_ptr, feed_cstr.as_ptr(), title_ptr, author_ptr);
    }

    // Snapshot count after — if it grew, a new row was added.
    let after_len = snapshot_json_len(ph_ptr);
    if after_len > before_len { JNI_TRUE } else { JNI_FALSE }
}

/// `PodcastKernelBridge.nativeUnsubscribe(podcastHandle: Long, podcastId: String)`
#[no_mangle]
pub extern "system" fn Java_com_podcast_app_android_bridge_PodcastKernelBridge_nativeUnsubscribe<
    'l,
>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    podcast_handle: jlong,
    podcast_id: JString<'l>,
) {
    if podcast_handle == 0 {
        return;
    }
    let Some(id_cstr) = jstring_to_cstring(&mut env, podcast_id) else {
        return;
    };
    unsafe {
        nmp_app_podcast_unsubscribe(podcast_handle as *mut _, id_cstr.as_ptr());
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a JString to an owned CString. Returns None if the JString is null
/// / empty or if JNI extraction fails (D6 silent degrade).
fn jstring_to_cstring(env: &mut JNIEnv<'_>, js: JString<'_>) -> Option<CString> {
    if js.is_null() {
        return None;
    }
    let rust_str: String = env.get_string(&js).ok()?.into();
    if rust_str.is_empty() {
        return None;
    }
    CString::new(rust_str).ok()
}

/// Returns the byte length of the snapshot JSON, or 0 on any failure.
/// Used as a lightweight change-detector around `nativeSubscribe`.
fn snapshot_json_len(ph_ptr: *mut nmp_app_podcast::PodcastHandle) -> usize {
    if ph_ptr.is_null() {
        return 0;
    }
    let ptr: *mut c_char = unsafe { nmp_app_podcast_snapshot(ph_ptr) };
    if ptr.is_null() {
        return 0;
    }
    let len = unsafe { CStr::from_ptr(ptr) }.to_bytes().len();
    unsafe { nmp_app_podcast_snapshot_free(ptr) };
    len
}
