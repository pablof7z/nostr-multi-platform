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
//! # URI-adapter symbols (still JNI — UniFFI NmpApp takes raw keys, not URIs)
//!
//! * [`nativeResolveEventRef`] — decodes a `nostr:` URI, calls
//!   `nmp_app_resolve_event_embed_with_metadata` on the inner NmpApp.
//! * [`nativeReleaseEvent`] — decodes a `nostr:` URI key, calls
//!   `nmp_app_release_event_ref`.
//!
//! Both URI adapter symbols accept an `arcPtr: Long` (a Kotlin
//! `Pointer.nativeValue(app.uniffiClonePointer())` result) so they operate
//! directly on the caller-owned UniFFI Arc — no process-global app pointer.

use std::ffi::{CStr, CString};
use std::sync::{Mutex, OnceLock};

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;

// ── Process-global ref-stores (nativeDecodeSnapshotJson) ─────────────────

static GALLERY_REF_STORES: OnceLock<Mutex<crate::GalleryRefStores>> = OnceLock::new();

fn get_ref_stores() -> &'static Mutex<crate::GalleryRefStores> {
    GALLERY_REF_STORES.get_or_init(|| Mutex::new(crate::GalleryRefStores::new()))
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn jstring_to_cstring(env: &mut JNIEnv, value: &JString) -> Option<CString> {
    let s = env.get_string(value).ok()?;
    CString::new(s.to_string_lossy().into_owned()).ok()
}

struct EventRefFromUri {
    key: CString,
    metadata_json: CString,
}

/// Decode a `nostr:` URI into a raw event key plus resolver metadata JSON.
/// D6: non-event URIs or decode failures return `None`.
fn event_ref_from_uri(uri: &CStr) -> Option<EventRefFromUri> {
    let raw = nmp_ffi::nmp_nip21_decode_uri(uri.as_ptr());
    if raw.is_null() {
        return None;
    }
    // SAFETY: non-null raw is a valid C string from nmp_nip21_decode_uri.
    let s = unsafe { CStr::from_ptr(raw) }
        .to_str()
        .ok()
        .map(str::to_owned);
    nmp_ffi::nmp_free_string(raw);
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
/// and `refs.event` row-delta batches across frames (ADR-0063 / #1671 —
/// sidecars carry only changed/cleared rows; a single frame cannot be decoded
/// in isolation). D6: null/empty/malformed input returns null.
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

/// Decode a `nostr:` URI and resolve the event embed with relay metadata.
///
/// `arc_ptr` is a `Pointer.nativeValue(app.uniffiClonePointer())` result from
/// Kotlin. `Arc::from_raw` takes ownership of the clone; the Arc drops at the
/// end of the function, decrementing the refcount back to the caller's 1.
///
/// D6: a zero `arc_ptr`, unparseable URI, or any JNI failure is a silent no-op.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeResolveEventRef(
    mut env: JNIEnv,
    _class: JClass,
    arc_ptr: jlong,
    uri: JString,
    consumer_id: JString,
) {
    if arc_ptr == 0 {
        return;
    }
    let Some(uri_cstr) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id_cstr) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    let Some(event_ref) = event_ref_from_uri(&uri_cstr) else {
        return;
    };
    // SAFETY: arc_ptr is a uniffiClonePointer() result (refcount bumped by 1).
    // Arc::from_raw takes ownership; drops at end decrement refcount to caller's 1.
    let arc = unsafe { std::sync::Arc::from_raw(arc_ptr as *const nmp_uniffi::NmpApp) };
    let inner_ptr = std::ptr::addr_of!(arc.inner) as *mut nmp_ffi::NmpApp;
    nmp_ffi::nmp_app_resolve_event_embed_with_metadata(
        inner_ptr,
        event_ref.key.as_ptr(),
        consumer_id_cstr.as_ptr(),
        event_ref.metadata_json.as_ptr(),
    );
    // arc drops here → refcount decremented back to caller's 1
}

/// Decode a `nostr:` URI and release the event ref.
///
/// Same Arc ownership contract as [`Java_org_nmp_gallery_bridge_KernelBridge_nativeResolveEventRef`].
/// D6: zero `arc_ptr`, bad URI, or JNI failure is a silent no-op.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "system" fn Java_org_nmp_gallery_bridge_KernelBridge_nativeReleaseEvent(
    mut env: JNIEnv,
    _class: JClass,
    arc_ptr: jlong,
    uri: JString,
    consumer_id: JString,
) {
    if arc_ptr == 0 {
        return;
    }
    let Some(uri_cstr) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    let Some(consumer_id_cstr) = jstring_to_cstring(&mut env, &consumer_id) else {
        return;
    };
    let Some(event_ref) = event_ref_from_uri(&uri_cstr) else {
        return;
    };
    // SAFETY: same as nativeResolveEventRef — uniffiClonePointer() result.
    let arc = unsafe { std::sync::Arc::from_raw(arc_ptr as *const nmp_uniffi::NmpApp) };
    let inner_ptr = std::ptr::addr_of!(arc.inner) as *mut nmp_ffi::NmpApp;
    nmp_ffi::nmp_app_release_event_ref(
        inner_ptr,
        event_ref.key.as_ptr(),
        consumer_id_cstr.as_ptr(),
    );
    // arc drops here → refcount decremented back to caller's 1
}
