//! Android JNI wrappers for the NIP-46 signer-broker seam.
//!
//! The broker is Rust-owned and initialised in `nativeNew`. Android reports
//! only user/platform facts: a pasted bunker URI, a cancel request, or an
//! optional callback scheme for a generated `nostrconnect://` URI.

use std::ffi::CStr;
use std::ptr;

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;

use nmp_app_chirp::{nmp_app_cancel_bunker_handshake, nmp_app_nostrconnect_uri};
use nmp_ffi::nmp_free_string;
use nmp_ffi::nmp_app_signin_bunker;

use crate::{jstring_to_cstring, session_arc};

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSignInBunker(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    uri: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(uri) = jstring_to_cstring(&mut env, &uri) else {
        return;
    };
    s.with_app(|app| nmp_app_signin_bunker(app, uri.as_ptr(), 1));
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCancelBunkerHandshake(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.with_app(|app| nmp_app_cancel_bunker_handshake(app));
    }
}

/// D3: relay selection is Rust-owned. The JNI bridge passes only the optional
/// platform callback scheme; the `relay_url` param was removed in #1615.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeNostrConnectUri(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    callback_scheme: JString,
) -> jstring {
    let Some(s) = session_arc(handle) else {
        return ptr::null_mut();
    };
    let callback_scheme = optional_jstring_to_cstring(&mut env, &callback_scheme);
    let Some(ptr) = s.with_app(|app| {
        nmp_app_nostrconnect_uri(
            app,
            callback_scheme
                .as_ref()
                .map_or(ptr::null(), |value| value.as_ptr()),
        )
    }) else {
        return ptr::null_mut();
    };
    if ptr.is_null() {
        return ptr::null_mut();
    }
    let uri = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_free_string(ptr);
    env.new_string(uri)
        .map(|value| value.into_raw())
        .unwrap_or(ptr::null_mut())
}

fn optional_jstring_to_cstring(env: &mut JNIEnv, value: &JString) -> Option<std::ffi::CString> {
    let obj: &JObject = AsRef::<JObject>::as_ref(value);
    if obj.as_raw().is_null() {
        return None;
    }
    jstring_to_cstring(env, value)
}
