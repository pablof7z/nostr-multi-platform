//! Android JNI wrappers for the demand-driven claim/release seam.
//!
//! `nativeClaimEvent` and `nativeReleaseEvent` are app-owned URI adapters: they
//! decode the `nostr:` URI in Rust and forward the raw event key plus decoded
//! metadata to the unified ref seam.
//!
//! Active claim families:
//!   * embedded-event claims — `nativeClaimEvent` / `nativeReleaseEvent`
//!     (now internally via the typed event-ref adapter)
//!
//! Doctrine: no business logic or cached state here (D5/D8) — the kernel owns
//! the claim ledger and resolution; these entrypoints forward strings and
//! return void. D6: a null/dead handle or a malformed string is a silent no-op.

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::jlong;

use nmp_ffi::{
    nmp_app_release_event_ref, nmp_app_resolve_event_embed_with_metadata, nmp_nip21_decode_uri,
};

use crate::{jstring_to_cstring, session_arc};

struct EventRefFromUri {
    key: std::ffi::CString,
    metadata_json: std::ffi::CString,
}

/// Decode a `nostr:` URI via `nmp_nip21_decode_uri` and return the canonical
/// raw event key plus metadata the kernel resolver expects:
///   - nevent / note  → the hex event_id
///   - naddr          → the canonical coordinate string "kind:pubkey:identifier"
/// Returns `None` on any failure (invalid URI, not an event/address target, etc.)
/// so callers silently no-op (D6).
fn event_ref_from_uri(uri: &std::ffi::CStr) -> Option<EventRefFromUri> {
    let raw = nmp_nip21_decode_uri(uri.as_ptr());
    if raw.is_null() {
        return None;
    }
    // SAFETY: non-null raw is a valid CStr produced by nmp_nip21_decode_uri.
    let s = unsafe { std::ffi::CStr::from_ptr(raw) }
        .to_str()
        .ok()
        .map(str::to_owned);
    nmp_ffi::nmp_free_string(raw);

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
    let relays: Vec<String> = v
        .get("relays")
        .and_then(|r| r.as_array())?
        .iter()
        .map(|relay| relay.as_str().map(str::to_owned))
        .collect::<Option<_>>()?;
    let mut metadata = serde_json::json!({ "hints": relays });
    if let Some(author) = v.get("author").and_then(|a| a.as_str()) {
        metadata["author"] = serde_json::Value::String(author.to_string());
    }
    if let Some(kind) = v.get("kind").and_then(|k| k.as_u64()) {
        metadata["kind"] = serde_json::Value::Number(kind.into());
    }
    Some(EventRefFromUri {
        key: std::ffi::CString::new(key).ok()?,
        metadata_json: std::ffi::CString::new(metadata.to_string()).ok()?,
    })
}

/// Demand-driven embedded-event claim (#984 / T180 / ADR-0034 / #1726).
///
/// Decodes the `nostr:` URI in Rust, extracts the event-id key, and forwards to
/// the typed event-embed ref adapter.
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
    let Some(event_ref) = event_ref_from_uri(&uri_cstr) else {
        return;
    };
    s.with_app(|app| {
        nmp_app_resolve_event_embed_with_metadata(
            app,
            event_ref.key.as_ptr(),
            consumer_id.as_ptr(),
            event_ref.metadata_json.as_ptr(),
        );
    });
}

/// Release a previously-claimed embedded event (#984 / #1726).
///
/// Decodes the `nostr:` URI in Rust, extracts the event-id key, and forwards to
/// the typed event-ref release adapter.
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
    let Some(event_ref) = event_ref_from_uri(&uri_cstr) else {
        return;
    };
    s.with_app(|app| {
        nmp_app_release_event_ref(app, event_ref.key.as_ptr(), consumer_id.as_ptr());
    });
}
