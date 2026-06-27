//! Android JNI event-ref URI adapter for NmpGallery.
//!
//! Decodes app-local `nostr:` URI inputs and forwards them through the raw-key
//! ref seam while preserving decoded relay/author metadata for event lookup.

use std::ffi::{CStr, CString};

use jni::objects::{JClass, JString};
use jni::sys::jlong;
use jni::JNIEnv;

use nmp_ffi::{
    nmp_app_release_event_ref, nmp_app_resolve_event_embed_with_metadata, nmp_free_string,
    nmp_nip21_decode_uri,
};

use super::session::{jstring_to_cstring, session_ref};

struct EventRefFromUri {
    key: CString,
    metadata_json: CString,
}

/// Decode a `nostr:` URI into the canonical event key plus resolver metadata.
/// D6: non-event URIs or decode failures become silent no-ops at the JNI edge.
fn event_ref_from_uri(uri: &CStr) -> Option<EventRefFromUri> {
    let raw = nmp_nip21_decode_uri(uri.as_ptr());
    if raw.is_null() {
        return None;
    }
    // SAFETY: non-null raw is a valid CStr from nmp_nip21_decode_uri.
    let s = unsafe { CStr::from_ptr(raw) }
        .to_str()
        .ok()
        .map(str::to_owned);
    nmp_free_string(raw);
    let s = s?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    if !v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false) {
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
        key: CString::new(key).ok()?,
        metadata_json: CString::new(metadata.to_string()).ok()?,
    })
}

/// App-local JNI URI adapter. Decodes the `nostr:` URI in Rust and forwards to
/// the typed event-embed ref adapter.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeResolveEventRef(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(uri_cstr) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    let Some(event_ref) = event_ref_from_uri(&uri_cstr) else {
        return;
    };
    nmp_app_resolve_event_embed_with_metadata(
        s.app,
        event_ref.key.as_ptr(),
        consumer_id.as_ptr(),
        event_ref.metadata_json.as_ptr(),
    );
}

/// App-local JNI URI adapter. Decodes the `nostr:` URI in Rust and forwards to
/// the typed event-ref release adapter.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeReleaseEvent(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
    consumer_id: JString,
) {
    let Some(s) = session_ref(handle) else { return };
    let Some(uri_cstr) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    let Some(event_ref) = event_ref_from_uri(&uri_cstr) else {
        return;
    };
    nmp_app_release_event_ref(s.app, event_ref.key.as_ptr(), consumer_id.as_ptr());
}
