//! Android JNI event-ref URI adapter for NmpGallery.
//!
//! Decodes app-local `nostr:` URI inputs and forwards them through the typed
//! event-ref seam while preserving decoded relay/author metadata for lookup.

use jni::objects::{JClass, JString};
use jni::sys::jlong;
use jni::JNIEnv;

use super::session::{jstring_to_cstring, session_ref};

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
    let Some(event_ref) = crate::event_ref_uri::event_ref_from_uri(&uri_cstr.to_string_lossy())
    else {
        return;
    };
    let metadata = gallery_event_metadata(&event_ref.metadata_json);
    s.app.resolve_ref_with_metadata(
        nmp_core::RefNamespace::Event,
        event_ref.key,
        consumer_id.to_string_lossy().into_owned(),
        nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
        nmp_core::RefLiveness::CacheOk,
        metadata,
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
    let Some(event_ref) = crate::event_ref_uri::event_ref_from_uri(&uri_cstr.to_string_lossy())
    else {
        return;
    };
    s.app.release_ref(
        nmp_core::RefNamespace::Event,
        event_ref.key,
        consumer_id.to_string_lossy().into_owned(),
    );
}

fn gallery_event_metadata(json: &str) -> nmp_core::RefResolveMetadata {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return nmp_core::RefResolveMetadata::default();
    };
    let hints = value
        .get("hints")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let event_author = value
        .get("author")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    nmp_core::RefResolveMetadata {
        hints,
        event_author,
    }
}
