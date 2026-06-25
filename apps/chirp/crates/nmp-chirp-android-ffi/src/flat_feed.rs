//! Android JNI entrypoints for Chirp feed-session lifecycle.

use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::ptr;

use jni::objects::{JClass, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;

use nmp_app_chirp::{nmp_app_close_feed, nmp_app_open_feed};
use nmp_feed::{
    FeedAdmission, FeedParams, FeedRanking, FeedRender, FeedScope, FeedWindow, ProjectionKey,
    DEFAULT_FEED_WINDOW_LIMIT,
};
use nmp_ffi::{nmp_free_string, NmpApp};
use serde_json::Value;

use crate::{jstring_to_cstring, session_arc};

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeOpenHomeFeed(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    open_for_session(&mut env, handle, home_feed_params())
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeOpenThread(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    note_id: JString,
) -> jstring {
    let Some(note_id) = jstring_to_string(&mut env, &note_id) else {
        return ptr::null_mut();
    };
    open_for_session(&mut env, handle, thread_feed_params(&note_id))
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCloseThread(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    feed_handle: JString,
) {
    close_for_session(&mut env, handle, &feed_handle);
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeOpenAuthor(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
) -> jstring {
    let Some(pubkey) = jstring_to_string(&mut env, &pubkey) else {
        return ptr::null_mut();
    };
    open_for_session(&mut env, handle, author_feed_params(&pubkey))
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCloseAuthor(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    feed_handle: JString,
) {
    close_for_session(&mut env, handle, &feed_handle);
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCloseFeed(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    feed_handle: JString,
) {
    close_for_session(&mut env, handle, &feed_handle);
}

fn open_for_session(env: &mut JNIEnv, handle: jlong, params: FeedParams) -> jstring {
    let Some(s) = session_arc(handle) else {
        return ptr::null_mut();
    };
    let Some(handle_json) = s.with_app(|app| open_feed_handle(app, &params)).flatten() else {
        return ptr::null_mut();
    };
    match env.new_string(handle_json) {
        Ok(value) => value.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

fn close_for_session(env: &mut JNIEnv, handle: jlong, feed_handle: &JString) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(feed_handle) = jstring_to_cstring(env, feed_handle) else {
        return;
    };
    s.with_app(|app| close_feed_handle(app, &feed_handle));
}

fn home_feed_params() -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::OpCentric,
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: default_window(),
        projection: ProjectionKey("nmp.feed.home".to_string()),
    }
}

fn author_feed_params(pubkey: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::Authors {
            authors: BTreeSet::from([pubkey.to_string()]),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: default_window(),
        projection: ProjectionKey(format!("nmp.feed.author.{pubkey}")),
    }
}

fn thread_feed_params(event_id: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::Referrer {
            event_id: event_id.to_string(),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: default_window(),
        projection: ProjectionKey(format!("nmp.feed.thread.{event_id}")),
    }
}

fn default_window() -> FeedWindow {
    FeedWindow {
        initial_limit: DEFAULT_FEED_WINDOW_LIMIT,
    }
}

fn open_feed_handle(app: *mut NmpApp, params: &FeedParams) -> Option<String> {
    if app.is_null() {
        return None;
    }
    let params_json = serde_json::to_string(params).ok()?;
    let params_c = CString::new(params_json).ok()?;
    let raw = nmp_app_open_feed(app, params_c.as_ptr());
    if raw.is_null() {
        return None;
    }
    let handle_json = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    nmp_free_string(raw);
    let value = serde_json::from_str::<Value>(&handle_json).ok()?;
    value.get("error").is_none().then_some(handle_json)
}

fn close_feed_handle(app: *mut NmpApp, handle_json: &CString) {
    if !app.is_null() {
        nmp_app_close_feed(app, handle_json.as_ptr());
    }
}

fn jstring_to_string(env: &mut JNIEnv, value: &JString) -> Option<String> {
    jstring_to_cstring(env, value).map(|s| s.to_string_lossy().into_owned())
}
