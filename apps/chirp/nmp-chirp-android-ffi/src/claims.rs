//! Android JNI wrappers for the demand-driven claim/release seam.
//!
//! #1726: `nativeClaimEvent` and `nativeReleaseEvent` now decode the `nostr:`
//! URI in Rust (via `nmp_nip21_decode_uri`) and forward the event key to the
//! unified `nmp_app_resolve_ref` / `nmp_app_release_ref` seam. The deleted
//! `nmp_app_claim_event` / `nmp_app_release_event` C-ABI symbols are NOT used
//! here (no compat shim kept — they are gone).
//!
//! Active claim families:
//!   * embedded-event claims — `nativeClaimEvent` / `nativeReleaseEvent`
//!     (now internally via resolve_ref / release_ref, namespace=1/event)
//!   * unified ref-resolver (ADR-0063) — `nativeResolveRef` / `nativeReleaseRef`
//!
//! Doctrine: no business logic or cached state here (D5/D8) — the kernel owns
//! the claim ledger and resolution; these entrypoints forward strings and
//! return void. D6: a null/dead handle or a malformed string is a silent no-op.

use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong};
use jni::JNIEnv;

use nmp_ffi::{nmp_app_release_ref, nmp_app_resolve_ref, nmp_nip21_decode_uri};

use crate::{jstring_to_cstring, session_arc};

/// Decode a `nostr:` URI via `nmp_nip21_decode_uri` and return the canonical
/// event key the kernel resolver expects:
///   - nevent / note  → the hex event_id
///   - naddr          → the canonical coordinate string "kind:pubkey:identifier"
/// Returns `None` on any failure (invalid URI, not an event/address target, etc.)
/// so callers silently no-op (D6).
fn event_key_from_uri(uri: &std::ffi::CStr) -> Option<std::ffi::CString> {
    // SAFETY: nmp_nip21_decode_uri is a pure C-ABI function that returns a
    // heap-allocated NUL-terminated JSON string (or a well-formed error JSON).
    let raw = unsafe { nmp_nip21_decode_uri(uri.as_ptr()) };
    if raw.is_null() {
        return None;
    }
    // SAFETY: non-null raw is a valid CStr produced by nmp_nip21_decode_uri.
    let s = unsafe { std::ffi::CStr::from_ptr(raw) }
        .to_str()
        .ok()
        .map(str::to_owned);
    // Free the heap string returned by nmp_nip21_decode_uri.
    // SAFETY: raw is from nmp_nip21_decode_uri and is freed exactly once here.
    unsafe { nmp_ffi::nmp_free_string(raw) };

    let s = s?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    let ok = v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false);
    if !ok {
        return None;
    }
    let key = match v.get("target").and_then(|t| t.as_str()) {
        Some("event") => v.get("event_id").and_then(|e| e.as_str())?.to_owned(),
        Some("address") => {
            let kind = v.get("kind").and_then(|k| k.as_u64())?;
            let pubkey = v.get("pubkey").and_then(|p| p.as_str())?;
            let identifier = v.get("identifier").and_then(|i| i.as_str())?;
            format!("{kind}:{pubkey}:{identifier}")
        }
        _ => return None,
    };
    std::ffi::CString::new(key).ok()
}

/// Demand-driven embedded-event claim (#984 / T180 / ADR-0034 / #1726).
///
/// #1726: decodes the `nostr:` URI in Rust, extracts the event-id key, and
/// forwards to `nmp_app_resolve_ref(namespace=1/event, shape=2/embed,
/// liveness=0/CacheOk)`. The deleted `nmp_app_claim_event` C-ABI symbol is
/// NOT called.
///
/// D6: bad handles / non-event URIs / decode errors are silent no-ops.
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
    let Some(uri_cstr) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    let Some(event_key) = event_key_from_uri(&uri_cstr) else {
        return;
    };
    s.with_app(|app| {
        nmp_app_resolve_ref(
            app,
            1, // namespace = event
            event_key.as_ptr(),
            consumer_id.as_ptr(),
            2, // shape = event.embed
            0, // liveness = CacheOk (background/auto-claim)
        );
    });
}

/// Release a previously-claimed embedded event (#984 / #1726).
///
/// #1726: decodes the `nostr:` URI in Rust, extracts the event-id key, and
/// forwards to `nmp_app_release_ref(namespace=1/event)`. The deleted
/// `nmp_app_release_event` C-ABI symbol is NOT called.
///
/// D6: bad handles / non-event URIs / decode errors are silent no-ops.
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
    let Some(uri_cstr) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    let Some(event_key) = event_key_from_uri(&uri_cstr) else {
        return;
    };
    s.with_app(|app| {
        nmp_app_release_ref(
            app,
            1, /*event*/
            event_key.as_ptr(),
            consumer_id.as_ptr(),
        );
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
