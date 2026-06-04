//! Android JNI wrappers for platform/lifecycle parity with iOS.
//!
//! Kotlin reports platform facts (storage directory, foreground/background,
//! visible feed tail). The kernel owns the meaning of each fact.

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jlong};
use jni::JNIEnv;

use nmp_ffi::{
    nmp_app_is_alive, nmp_app_lifecycle_background, nmp_app_lifecycle_foreground,
    nmp_app_load_older_feed, nmp_app_set_storage_path,
};

use crate::{jstring_to_cstring, session_ref};

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSetStoragePath(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    path: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(path) = jstring_to_cstring(&mut env, &path) else {
        return;
    };
    nmp_app_set_storage_path(s.app, path.as_ptr());
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeLifecycleForeground(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_ref(handle) {
        nmp_app_lifecycle_foreground(s.app);
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeLifecycleBackground(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_ref(handle) {
        nmp_app_lifecycle_background(s.app);
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeIsAlive(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jboolean {
    let Some(s) = session_ref(handle) else {
        return 0;
    };
    if nmp_app_is_alive(s.app) == 1 {
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
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(feed_key) = jstring_to_cstring(&mut env, &feed_key) else {
        return;
    };
    nmp_app_load_older_feed(s.app, feed_key.as_ptr());
}
