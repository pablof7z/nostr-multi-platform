//! Gallery-owned Android JNI surface — post M14 shell-2.
//!
//! After M14 shell-2 the NmpApp lifecycle (new / free / start / stop /
//! signin / relay / update-callback / capability-callback) is owned by the
//! UniFFI `NmpApp` Kotlin class. Only the gallery-specific JNI symbols that
//! have no UniFFI counterpart on the NmpApp interface remain here.
//!
//! # Gallery-owned JNI symbols
//!
//! * [`nativeShowcaseReferencesJson`] — static embedded showcase reference JSON.
//! * [`nativeRegistryJson`] — static embedded registry JSON.
//! * [`nativeDecodeSnapshotJson`] — FlatBuffers frame → gallery snapshot JSON.
//!   Uses a process-global [`GALLERY_REF_STORES`] because the gallery runs
//!   exactly one kernel session per process lifetime (ADR-0063 / #1671 —
//!   row-delta batches must accumulate across frames).
//!
//! # URI-adapter symbols
//!
//! * [`nativeEventRefFromUri`] — decodes a `nostr:` URI into the raw event key
//!   plus resolver metadata JSON. Kotlin then calls the typed UniFFI
//!   `resolveEventEmbedWithMetadata` / `releaseEventRef` methods.

use std::sync::{Mutex, OnceLock};

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;

// ── Process-global ref-stores (nativeDecodeSnapshotJson) ─────────────────

static GALLERY_REF_STORES: OnceLock<Mutex<crate::GalleryRefStores>> = OnceLock::new();

fn get_ref_stores() -> &'static Mutex<crate::GalleryRefStores> {
    GALLERY_REF_STORES.get_or_init(|| Mutex::new(crate::GalleryRefStores::new()))
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn jstring_to_string(env: &mut JNIEnv, value: &JString) -> Option<String> {
    let s = env.get_string(value).ok()?;
    Some(s.to_string_lossy().into_owned())
}

// ── Gallery-owned JNI symbols ────────────────────────────────────────────

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeShowcaseReferencesJson<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
) -> jstring {
    match env.new_string(crate::showcase::raw_json()) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeRegistryJson<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
) -> jstring {
    match env.new_string(crate::registry::raw_json()) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Decode one FlatBuffers snapshot frame to the gallery JSON shape.
///
/// Uses the process-global [`GALLERY_REF_STORES`] to accumulate `refs.profile`
/// and `refs.event` row-delta batches across frames (ADR-0063 / #1671).
/// D6: null/empty/malformed input returns null.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeDecodeSnapshotJson<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    frame: JByteArray<'l>,
) -> jstring {
    let null = std::ptr::null_mut();
    let Ok(bytes) = env.convert_byte_array(frame) else {
        return null;
    };
    let Ok(mut guard) = get_ref_stores().lock() else {
        return null;
    };
    let stores = &mut *guard;
    let Ok(json) = crate::snapshot_json::snapshot_json_from_update_frame(
        &bytes,
        &mut stores.profiles,
        &mut stores.events,
    ) else {
        return null;
    };
    drop(guard);
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => null,
    }
}

// ── URI-adapter symbols ───────────────────────────────────────────────────

/// Decode a `nostr:` URI into a raw event key and resolver metadata JSON.
///
/// D6: unparseable URI or JNI failure returns null. Kotlin owns the app-facing
/// resolve/release calls through typed UniFFI methods.
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeEventRefFromUri(
    mut env: JNIEnv,
    _class: JClass,
    uri: JString,
) -> jstring {
    let null = std::ptr::null_mut();
    let Some(uri) = jstring_to_string(&mut env, &uri) else {
        return null;
    };
    let Some(event_ref) = crate::event_ref_uri::event_ref_from_uri(&uri) else {
        return null;
    };
    let Ok(json) = serde_json::to_string(&serde_json::json!({
        "key": event_ref.key,
        "metadata_json": event_ref.metadata_json,
    })) else {
        return null;
    };
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => null,
    }
}
