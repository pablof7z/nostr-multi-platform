//! Android JNI entrypoints for Chirp's per-screen flat feeds.

use jni::objects::{JClass, JString};
use jni::sys::jlong;
use jni::JNIEnv;

use nmp_app_chirp::{
    nmp_app_chirp_close_author_feed, nmp_app_chirp_close_thread_feed,
    nmp_app_chirp_open_author_feed, nmp_app_chirp_open_thread_feed,
};

use crate::{jstring_to_cstring, session_ref};

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeOpenThread(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    note_id: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(note_id) = jstring_to_cstring(&mut env, &note_id) else {
        return;
    };
    nmp_app_chirp_open_thread_feed(s.app, note_id.as_ptr());
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCloseThread(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    note_id: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(note_id) = jstring_to_cstring(&mut env, &note_id) else {
        return;
    };
    nmp_app_chirp_close_thread_feed(s.app, note_id.as_ptr());
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeOpenAuthor(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    nmp_app_chirp_open_author_feed(s.app, pubkey.as_ptr());
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCloseAuthor(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
) {
    let Some(s) = session_ref(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    nmp_app_chirp_close_author_feed(s.app, pubkey.as_ptr());
}
