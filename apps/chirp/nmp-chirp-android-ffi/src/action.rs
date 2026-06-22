//! Android JNI wrappers for the Rust-owned action seam.
//!
//! The bridge parses no action vocabulary. Kotlin supplies a namespace and
//! JSON body, Rust validates/executes through `nmp_app_dispatch_action`, and
//! terminal stage cleanup goes back through `nmp_app_ack_action_stage`.

use jni::objects::{JClass, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;

use nmp_ffi::{
    nmp_app_ack_action_stage, nmp_app_cancel_action, nmp_app_dispatch_action, nmp_app_retry_publish,
    nmp_free_string,
};

use crate::{jstring_to_cstring, session_arc};

/// Return `value` as a JNI `jstring`, falling back to a null pointer on any
/// JNI failure (D6 — errors must never cross the FFI seam as a panic).
///
/// The previous fallback `env.new_string("{}").unwrap()` could itself panic
/// (e.g. when the JVM is shutting down or the local-ref table is exhausted),
/// propagating through `extern "system"` — undefined behaviour per D6.
fn json_string(env: JNIEnv, value: &str) -> jstring {
    env.new_string(value)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
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
    nmp_free_string(result_ptr);
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

/// Retry a failed publish identified by its correlation id (outbox UI).
/// Control-plane only: Rust owns the publish ledger and re-enqueues the event;
/// Kotlin forwards the handle string verbatim. D6: a null handle / malformed
/// JNI argument is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeRetryPublish(
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
    s.with_app(|app| nmp_app_retry_publish(app, correlation_id.as_ptr()));
}

/// Cancel an in-flight publish identified by its operation `correlation_id`
/// (outbox UI). Control-plane only: Rust owns the publish ledger, reverse-
/// resolves the publish handle from the durable handle↔correlation index, and
/// records the user-initiated `Cancelled` terminal under the ORIGINAL
/// correlation_id (S7/#1754, PD-036). Kotlin forwards the correlation_id string
/// verbatim. D6: a null / malformed JNI argument is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCancelPublish(
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
    s.with_app(|app| nmp_app_cancel_action(app, correlation_id.as_ptr()));
}
