use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Arc;

use nmp_native_runtime::{NmpApp as NativeRuntimeApp, UpdateListener};
use zeroize::Zeroizing;

use crate::{dispatch_bytes, json_escape, register_gallery_composition};

pub struct GalleryNativeApp {
    app: NativeRuntimeApp,
}

impl GalleryNativeApp {
    fn new() -> Self {
        let mut app = nmp_native_runtime::new_app();
        let installed = register_gallery_composition(&mut app);
        debug_assert!(
            installed,
            "fresh GalleryNativeApp must install its composition root"
        );
        // ADR-0053 / Workstream-E4 — mirrors nmp_app_gallery_register and
        // nmp_app_gallery_register_uniffi: the gallery showcases the full
        // built-in component set, so it must declare that read intent before
        // start_runtime, or NmpApp::start_runtime aborts (undeclared intent).
        app.consume_all_builtin_projections();
        Self { app }
    }
}

type GalleryUpdateCallback = extern "C" fn(context: *mut c_void, bytes: *const u8, len: usize);

#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_new() -> *mut GalleryNativeApp {
    Box::into_raw(Box::new(GalleryNativeApp::new()))
}

/// # Safety
/// `app` must be a pointer returned by [`nmp_gallery_kernel_new`] and not
/// already freed, or null.
#[no_mangle]
pub unsafe extern "C" fn nmp_gallery_kernel_free(app: *mut GalleryNativeApp) {
    if app.is_null() {
        return;
    }
    let session = unsafe { Box::from_raw(app) };
    session.app.shutdown();
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_set_update_callback(
    app: *mut GalleryNativeApp,
    context: *mut c_void,
    callback: Option<GalleryUpdateCallback>,
) {
    let Some(session) = gallery_session_ref(app) else {
        return;
    };
    let listener: Option<UpdateListener> = callback.map(|callback| {
        let context = context as usize;
        Arc::new(move |bytes: &[u8]| {
            callback(context as *mut c_void, bytes.as_ptr(), bytes.len());
        }) as UpdateListener
    });
    session.app.set_update_listener(listener);
}

#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_start(
    app: *mut GalleryNativeApp,
    visible_limit: u32,
    emit_hz: u32,
) {
    if let Some(session) = gallery_session_ref(app) {
        session
            .app
            .start_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
    }
}

#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_stop(app: *mut GalleryNativeApp) {
    if let Some(session) = gallery_session_ref(app) {
        session.app.stop_runtime();
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_set_storage_path(
    app: *mut GalleryNativeApp,
    path: *const c_char,
) -> u32 {
    let Some(session) = gallery_session_ref(app) else {
        return nmp_native_runtime::NmpConfigStatus::NullApp as u32;
    };
    let path = if path.is_null() {
        None
    } else {
        let value = unsafe { CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned();
        Some(value)
    };
    session.app.set_storage_path(path) as u32
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_add_relay(
    app: *mut GalleryNativeApp,
    url: *const c_char,
    role: *const c_char,
) {
    let Some(session) = gallery_session_ref(app) else {
        return;
    };
    let Some(url) = c_string(url) else { return };
    let role = c_string(role).unwrap_or_else(|| "both".to_string());
    session.app.add_relay(url, role);
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_resolve_profile_ref(
    app: *mut GalleryNativeApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    resolve_profile(app, key, consumer_id, nmp_core::ProfileShape::Ref);
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_resolve_profile_card(
    app: *mut GalleryNativeApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    resolve_profile(app, key, consumer_id, nmp_core::ProfileShape::Card);
}

fn resolve_profile(
    app: *mut GalleryNativeApp,
    key: *const c_char,
    consumer_id: *const c_char,
    shape: nmp_core::ProfileShape,
) {
    let Some(session) = gallery_session_ref(app) else {
        return;
    };
    let (Some(key), Some(consumer_id)) = (c_string(key), c_string(consumer_id)) else {
        return;
    };
    session.app.resolve_ref(
        nmp_core::RefNamespace::Profile,
        key,
        consumer_id,
        nmp_core::RefShape::Profile(shape),
        nmp_core::RefLiveness::CacheOk,
    );
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_release_profile_ref(
    app: *mut GalleryNativeApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(session) = gallery_session_ref(app) else {
        return;
    };
    let (Some(key), Some(consumer_id)) = (c_string(key), c_string(consumer_id)) else {
        return;
    };
    session
        .app
        .release_ref(nmp_core::RefNamespace::Profile, key, consumer_id);
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_resolve_event_embed_with_metadata(
    app: *mut GalleryNativeApp,
    key: *const c_char,
    consumer_id: *const c_char,
    metadata_json: *const c_char,
) {
    resolve_event_embed(
        app,
        key,
        consumer_id,
        metadata_json,
        nmp_core::RefLiveness::CacheOk,
    );
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_resolve_event_embed_live_with_metadata(
    app: *mut GalleryNativeApp,
    key: *const c_char,
    consumer_id: *const c_char,
    metadata_json: *const c_char,
) {
    resolve_event_embed(
        app,
        key,
        consumer_id,
        metadata_json,
        nmp_core::RefLiveness::Live,
    );
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_release_event_ref(
    app: *mut GalleryNativeApp,
    key: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(session) = gallery_session_ref(app) else {
        return;
    };
    let (Some(key), Some(consumer_id)) = (c_string(key), c_string(consumer_id)) else {
        return;
    };
    session
        .app
        .release_ref(nmp_core::RefNamespace::Event, key, consumer_id);
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_signin_nsec(
    app: *mut GalleryNativeApp,
    secret: *const c_char,
    make_active: u8,
) {
    let Some(session) = gallery_session_ref(app) else {
        return;
    };
    let Some(secret) = c_string(secret) else {
        return;
    };
    session.app.add_signer(
        nmp_core::SignerSource::LocalNsec(Zeroizing::new(secret)),
        make_active != 0,
    );
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_gallery_kernel_dispatch_action_bytes(
    app: *mut GalleryNativeApp,
    namespace: *const c_char,
    body_json: *const c_char,
) -> *mut c_char {
    let Some(session) = gallery_session_ref(app) else {
        return error_string("runtime app is not available");
    };
    let namespace = c_string(namespace).unwrap_or_default();
    let body_json = c_string(body_json).unwrap_or_default();
    let app_ptr = std::ptr::addr_of!(session.app) as *mut NativeRuntimeApp;
    let envelope = match dispatch_bytes::dispatch_action_bytes_for(app_ptr, &namespace, &body_json)
    {
        Ok(envelope) => envelope,
        Err(error) => format!(r#"{{"error":{}}}"#, json_escape(&error)),
    };
    CString::new(envelope)
        .unwrap_or_else(|_| {
            CString::new(r#"{"error":"dispatch result encoding failed"}"#).unwrap_or_default()
        })
        .into_raw()
}

fn gallery_session_ref<'a>(app: *mut GalleryNativeApp) -> Option<&'a GalleryNativeApp> {
    if app.is_null() {
        None
    } else {
        Some(unsafe { &*app })
    }
}

fn c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn error_string(message: &str) -> *mut c_char {
    CString::new(format!(r#"{{"error":{}}}"#, json_escape(message)))
        .unwrap_or_default()
        .into_raw()
}

fn resolve_event_embed(
    app: *mut GalleryNativeApp,
    key: *const c_char,
    consumer_id: *const c_char,
    metadata_json: *const c_char,
    liveness: nmp_core::RefLiveness,
) {
    let Some(session) = gallery_session_ref(app) else {
        return;
    };
    let (Some(key), Some(consumer_id)) = (c_string(key), c_string(consumer_id)) else {
        return;
    };
    let metadata = c_string(metadata_json)
        .and_then(|json| gallery_event_metadata(&json))
        .unwrap_or_default();
    session.app.resolve_ref_with_metadata(
        nmp_core::RefNamespace::Event,
        key,
        consumer_id,
        nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
        liveness,
        metadata,
    );
}

fn clamp_visible(visible_limit: u32) -> usize {
    if visible_limit == 0 {
        nmp_core::__ffi_internal::DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

fn clamp_emit_hz(emit_hz: u32) -> u32 {
    if emit_hz == 0 {
        nmp_core::__ffi_internal::DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}

fn gallery_event_metadata(json: &str) -> Option<nmp_core::RefResolveMetadata> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
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
    Some(nmp_core::RefResolveMetadata {
        hints,
        event_author,
    })
}
