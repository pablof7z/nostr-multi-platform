//! Android JNI event-ref URI adapter for NmpGallery.
//!
//! Decodes app-local `nostr:` URI inputs and forwards them through the typed
//! event-ref seam while preserving decoded relay/author metadata for lookup.

use std::ffi::CString;

use jni::objects::{JClass, JString};
use jni::sys::jlong;
use jni::JNIEnv;

use nmp_ffi::{nmp_app_release_event_ref, nmp_app_resolve_event_embed_with_metadata};

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
    let Ok(key) = CString::new(event_ref.key) else {
        return;
    };
    let Ok(metadata_json) = CString::new(event_ref.metadata_json) else {
        return;
    };
    nmp_app_resolve_event_embed_with_metadata(
        s.app,
        key.as_ptr(),
        consumer_id.as_ptr(),
        metadata_json.as_ptr(),
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
    let Ok(key) = CString::new(event_ref.key) else {
        return;
    };
    nmp_app_release_event_ref(s.app, key.as_ptr(), consumer_id.as_ptr());
}
