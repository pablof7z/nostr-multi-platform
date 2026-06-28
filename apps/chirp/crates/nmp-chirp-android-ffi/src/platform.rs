//! Android JNI wrappers for platform/lifecycle parity with iOS.
//!
//! Kotlin reports platform facts (storage directory, foreground/background,
//! visible feed tail). The kernel owns the meaning of each fact.

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong};
use jni::JNIEnv;

use nmp_ffi::{
    NmpConfigStatus,
    nmp_app_is_alive, nmp_app_lifecycle_background, nmp_app_lifecycle_foreground,
    nmp_app_load_older_feed, nmp_app_set_storage_path,
};

use crate::{jstring_to_cstring, session_arc};

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSetStoragePath(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    path: JString,
) -> jint {
    let Some(s) = session_arc(handle) else {
        return NmpConfigStatus::NullApp as jint;
    };
    let Some(path) = jstring_to_cstring(&mut env, &path) else {
        return NmpConfigStatus::Unavailable as jint;
    };
    s.with_app(|app| nmp_app_set_storage_path(app, path.as_ptr()) as jint)
        .unwrap_or(NmpConfigStatus::NullApp as jint)
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeLifecycleForeground(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.with_app(|app| nmp_app_lifecycle_foreground(app));
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeLifecycleBackground(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.with_app(|app| nmp_app_lifecycle_background(app));
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeIsAlive(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jboolean {
    let Some(s) = session_arc(handle) else {
        return 0;
    };
    if s.with_app(|app| nmp_app_is_alive(app)).unwrap_or(0) == 1 {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeLoadOlderFeed(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    feed_key: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(feed_key) = jstring_to_cstring(&mut env, &feed_key) else {
        return;
    };
    s.with_app(|app| nmp_app_load_older_feed(app, feed_key.as_ptr()));
}
