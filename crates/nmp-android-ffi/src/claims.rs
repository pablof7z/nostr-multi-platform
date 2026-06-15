//! Android JNI wrappers for the demand-driven claim/release seam.
//!
//! Two claim families share the same reference-counted, cache-first-then-relay
//! contract on the kernel side:
//!   * profile (kind:0) claims — `nmp_app_claim_profile` / `nmp_app_release_profile`
//!   * embedded-event claims (#984 / T180 / ADR-0034) — `nmp_app_claim_event` /
//!     `nmp_app_release_event`
//!
//! Doctrine: no business logic or cached state here (D5/D8) — the kernel owns
//! the claim ledger and resolution; these entrypoints forward strings and
//! return void. D6: a null/dead handle or a malformed string is a silent no-op.
//! F-TTL: every Android JNI claim is a background/auto-claim, so `force = 0`
//! (the lazy, TTL-gated path); user-navigation force-refresh is not exposed on
//! the Android bridge.

use jni::objects::{JClass, JString};
use jni::sys::jlong;
use jni::JNIEnv;

use nmp_ffi::{
    nmp_app_claim_event, nmp_app_claim_profile, nmp_app_release_event, nmp_app_release_profile,
};

use crate::{jstring_to_cstring, session_arc};

/// Demand-driven profile fetch claim. D6: bad handles/strings are no-ops.
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
    s.with_app(|app| {
        // force=0 (lazy F-TTL), liveness=0 (CacheOk — OneShot fetch on miss,
        // no tailing sub). The Android bridge does not yet surface either hint
        // to the Java layer; a future JNI param can opt list rows vs. the
        // profile screen into `Live` the same way iOS does.
        nmp_app_claim_profile(app, pubkey.as_ptr(), consumer_id.as_ptr(), 0, 0);
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

/// Demand-driven embedded-event claim (#984 / T180 / ADR-0034). The UI is
/// rendering an out-of-feed `EventRef` (`nevent`/`note`/`naddr`) under
/// `consumer_id`; the kernel resolves the event (cache-first, then relay) and
/// ships its typed projection in the next `NEMB` sidecar. Mirrors the gallery
/// app's `nativeClaimEvent` and the iOS `nmp_app_claim_event` call site.
/// D6: bad handles/strings are no-ops.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeClaimEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
    consumer_id: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(uri) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    s.with_app(|app| {
        nmp_app_claim_event(app, uri.as_ptr(), consumer_id.as_ptr(), 0);
    });
}

/// Release a previously-claimed embedded event (#984). Decrements the
/// per-consumer refcount in the kernel's `event_claims` table; the kernel drops
/// the resolution interest when the set is empty. Same contract as
/// `nmp_app_release_profile`. D6: bad handles/strings are no-ops.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeReleaseEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
    consumer_id: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(uri) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    s.with_app(|app| {
        nmp_app_release_event(app, uri.as_ptr(), consumer_id.as_ptr());
    });
}
