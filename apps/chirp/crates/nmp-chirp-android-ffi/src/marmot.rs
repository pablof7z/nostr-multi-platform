//! Marmot (MLS-over-Nostr encrypted groups) JNI entry points.
//!
//! Mirror of iOS `KernelHandle.registerActiveMarmotIfAvailable()` /
//! `unregisterMarmotIfNeeded()` (Bridge/MarmotBridge.swift). Registration uses
//! the actor-owned active local key — the secret never crosses the JNI seam
//! (`nmp_marmot_register_active` reads it from the slot the kernel writes after
//! every identity mutation). Once registered, the kernel pushes the
//! `nmp.marmot.snapshot` / `nmp.marmot.messages` projections on every snapshot
//! frame (V-107 / ADR-0039) and Kotlin reads Marmot state reactively from them;
//! write ops (create_group / invite / send / accept_welcome …) route through the
//! already-wired generic `nativeDispatchAction("nmp.marmot", …)` seam in
//! `lib.rs` — no bespoke per-op JNI symbol.
//!
//! WHY this module exists at all: calling `nmp_marmot_register_active` /
//! `nmp_marmot_unregister` through the `nmp_app_chirp::` RUST path (rather than
//! an `extern "C"` block) is what makes rustc pull the `nmp_marmot_*` symbol
//! bodies into the cdylib — the same retention mechanism documented at the top
//! of `lib.rs` for the `nmp_app_*` family.
//!
//! When the `marmot` feature is off (e.g. a plain `cargo build`), these entry
//! points still exist so the Kotlin `external fun` bindings link, but
//! registration is a no-op returning `false` (D6).

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jlong};
use jni::JNIEnv;

use crate::{jstring_to_cstring, session_arc, Session};

// Re-exported by `nmp-app-chirp` under its `marmot` feature (forwarded here by
// the `nmp-chirp-android-ffi/marmot` feature). Reached through the Rust path for
// symbol retention (see module doc).
#[cfg(feature = "marmot")]
use nmp_app_chirp::{nmp_marmot_register_active, nmp_marmot_unregister, MarmotHandle};

/// Register a Marmot MLS projection against the active local account.
///
/// `db_dir` is the host app-support directory; the MLS SQLite state lives at
/// `<db_dir>/marmot-mls-state.sqlite`. Returns `true` (1) when a handle was
/// obtained, `false` (0) otherwise (no local key — e.g. signed out or a
/// bunker/NIP-46 account — null `db_dir`, or the `marmot` feature disabled).
///
/// Idempotent: any handle from a prior call is unregistered first, so this
/// doubles as the account-switch re-register path (mirrors the
/// `unregisterMarmotIfNeeded()` that opens every iOS register helper).
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeMarmotRegisterActive(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    db_dir: JString,
) -> jboolean {
    let Some(s) = session_arc(handle) else {
        return 0;
    };
    let Some(dir) = jstring_to_cstring(&mut env, &db_dir) else {
        return 0;
    };
    register_active(&s, &dir) as jboolean
}

/// Drop the Marmot observer registration if one exists. Idempotent — a no-op
/// when no handle is registered or the `marmot` feature is disabled. Used by
/// the host sign-out path; `nativeFree` also performs this implicitly before
/// reclaiming the kernel.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeMarmotUnregister(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        unregister(&s);
    }
}

/// Register Marmot against the active local key, swapping out (and freeing)
/// any prior handle first. Returns whether a non-null handle is now stored.
#[cfg(feature = "marmot")]
fn register_active(s: &Session, db_dir: &std::ffi::CStr) -> bool {
    use std::ffi::CString;
    use std::sync::atomic::Ordering;
    // Re-register cleanly: tear down a stale handle before installing a fresh
    // one (account switch / re-sign-in), mirroring iOS.
    unregister(s);
    // The keyring service id is Chirp product policy (D0); it lives in
    // `nmp-chirp-config` rather than in the reusable `nmp-marmot` crate.
    let svc = CString::new(nmp_chirp_config::CHIRP_MARMOT_KEYRING_SERVICE_ID)
        .expect("static ASCII string is valid CStr");
    // `db_dir` is a valid NUL-terminated C string for the duration of this
    // call. The guarded app accessor returns `None` after close/free, making
    // late registration a no-op instead of racing native teardown.
    s.with_app(|app| {
        let new_handle = nmp_marmot_register_active(app, db_dir.as_ptr(), svc.as_ptr());
        if new_handle.is_null() {
            return false;
        }
        s.marmot
            .store(new_handle as *mut std::ffi::c_void, Ordering::SeqCst);
        true
    })
    .unwrap_or(false)
}

#[cfg(not(feature = "marmot"))]
fn register_active(_s: &Session, _db_dir: &std::ffi::CStr) -> bool {
    false
}

/// Unregister and free the stored Marmot handle, if any. Idempotent. Called by
/// `nativeFree` in `lib.rs` BEFORE `nmp_app_free` (the Marmot FFI contract
/// requires `nmp_marmot_unregister` to run first).
#[cfg(feature = "marmot")]
pub(crate) fn unregister(s: &Session) {
    use std::sync::atomic::Ordering;
    let prev = s.marmot.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if !prev.is_null() {
        // `prev` came from `nmp_marmot_register_active` and is swapped out
        // atomically, so it is unregistered exactly once. `nmp_marmot_unregister`
        // is a safe `extern "C" fn` (idempotent, null-guarded — D6).
        nmp_marmot_unregister(prev as *mut MarmotHandle);
    }
}

#[cfg(not(feature = "marmot"))]
pub(crate) fn unregister(_s: &Session) {}
