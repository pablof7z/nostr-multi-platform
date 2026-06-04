//! Android JNI wrappers for the Rust-owned action seam.
//!
//! The bridge parses no action vocabulary. Kotlin supplies a namespace and
//! JSON body, Rust validates/executes through `nmp_app_dispatch_action`, and
//! terminal stage cleanup goes back through `nmp_app_ack_action_stage`.

use jni::objects::{JClass, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;

use nmp_ffi::{nmp_app_ack_action_stage, nmp_app_dispatch_action, nmp_app_free_string};

use crate::{jstring_to_cstring, session_arc};

fn json_string(env: JNIEnv, value: &str) -> jstring {
    env.new_string(value)
        .unwrap_or_else(|_| env.new_string("{}").unwrap())
        .into_raw()
}

/// Dispatch a named action through the action registry.
///
/// Returns the Rust JSON envelope as a JNI string:
/// * `{"correlation_id":"<32-hex>"}` — accepted and enqueued.
/// * `{"error":"<message>"}` — rejected before execution.
///
/// D6: null handle or malformed JNI arguments collapse to `"{}"`; Kotlin
/// treats that as a parse failure rather than pretending the action succeeded.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeDispatchAction(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    namespace: JString,
    action_json: JString,
) -> jstring {
    let Some(s) = session_arc(handle) else {
        return json_string(env, "{}");
    };
    let Some(namespace) = jstring_to_cstring(&mut env, &namespace) else {
        return json_string(env, "{}");
    };
    let Some(action_json) = jstring_to_cstring(&mut env, &action_json) else {
        return json_string(env, "{}");
    };

    let Some(result_ptr) =
        s.with_app(|app| nmp_app_dispatch_action(app, namespace.as_ptr(), action_json.as_ptr()))
    else {
        return json_string(env, "{}");
    };
    if result_ptr.is_null() {
        return json_string(env, "{}");
    }

    let result = unsafe { std::ffi::CStr::from_ptr(result_ptr) }
        .to_string_lossy()
        .into_owned();
    nmp_app_free_string(result_ptr);
    json_string(env, &result)
}

/// Acknowledge that Android has reacted to a terminal `action_stages` entry.
/// Rust owns the stage ledger; this JNI symbol only forwards the correlation id.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeAckActionStage(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    correlation_id: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(correlation_id) = jstring_to_cstring(&mut env, &correlation_id) else {
        return;
    };
    s.with_app(|app| nmp_app_ack_action_stage(app, correlation_id.as_ptr()));
}
