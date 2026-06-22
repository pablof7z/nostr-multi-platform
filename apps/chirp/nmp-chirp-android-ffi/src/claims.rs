//! Android JNI wrappers for the demand-driven claim/release seam.
//!
//! ADR-0063 Lane H: profile (kind:0) legacy JNI wrappers deleted.
//! Active claim families:
//!   * embedded-event claims (#984 / T180 / ADR-0034) — `nmp_app_claim_event` /
//!     `nmp_app_release_event`
//!   * unified ref-resolver (ADR-0063) — `nmp_app_resolve_ref` / `nmp_app_release_ref`
//!
//! Doctrine: no business logic or cached state here (D5/D8) — the kernel owns
//! the claim ledger and resolution; these entrypoints forward strings and
//! return void. D6: a null/dead handle or a malformed string is a silent no-op.
//! F-TTL: every Android JNI claim is a background/auto-claim, so `force = 0`
//! (the lazy, TTL-gated path); user-navigation force-refresh is not exposed on
//! the Android bridge.

use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong};
use jni::JNIEnv;

use nmp_ffi::{
    nmp_app_claim_event, nmp_app_release_event, nmp_app_release_ref, nmp_app_resolve_ref,
};

use crate::{jstring_to_cstring, session_arc};

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
/// the resolution interest when the set is empty. D6: bad handles/strings are no-ops.
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

// ── ADR-0063 Lane D — unified resolve_ref / release_ref JNI surface ──────────

/// ADR-0063 Lane D — unified, origin-blind reference-resolution entry point.
///
/// `namespace` — 0 = profile, 1 = event.
/// `key` — lowercase 64-hex pubkey (profile) or lowercase event-id hex / `"kind:pubkey:d"` (event).
/// `consumer_id` — opaque refcount owner key (e.g. Compose LazyList item key).
/// `shape` — 0=profile.ref 1=profile.card 2=event.embed 3=event.raw.
/// `liveness` — 0=CacheOk (background), non-zero=Live (open screen).
///
/// Android shells call this instead of `nativeClaimEvent` for profile refs,
/// and for event refs use this in place of `nativeClaimEvent` where typed shape/liveness
/// control is needed. ADR-0063 Lane H: nativeClaimProfile deleted; use this.
/// D6: bad handles/strings/unknown int codes are silent no-ops.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeResolveRef(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    namespace: jint,
    key: JString,
    consumer_id: JString,
    shape: jint,
    liveness: jint,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(key) = jstring_to_cstring(&mut env, &key) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    s.with_app(|app| {
        nmp_app_resolve_ref(
            app,
            namespace,
            key.as_ptr(),
            consumer_id.as_ptr(),
            shape,
            liveness,
        );
    });
}

/// ADR-0063 Lane D — release a reference previously registered via
/// `nativeResolveRef`. Decrements the per-consumer refcount; the resolver
/// slot is torn down when the last consumer releases.
/// D6: bad handles/strings/unknown int codes are silent no-ops.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeReleaseRef(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    namespace: jint,
    key: JString,
    consumer_id: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(key) = jstring_to_cstring(&mut env, &key) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    s.with_app(|app| {
        nmp_app_release_ref(app, namespace, key.as_ptr(), consumer_id.as_ptr());
    });
}
