//! Android JNI wrappers for the Rust-owned action seam.
//!
//! NOTE (M14-0 / issue #2129): `nativeDispatchIntentBytes` and
//! `nativeDispatchActionBytes` have been **deleted** — action dispatch for the
//! app-loop lane is now served by `AppHandle::dispatch_action_json` /
//! `dispatch_action_bytes` in `uniffi_app_loop.rs`. Social writes go through the
//! generated `GeneratedActionBuilders` bytes → `dispatch_action_bytes` (M14-1 /
//! issue #2145); the `ChirpActionIntent` JSON lane has been retired.
//!
//! Retained symbols: `nativeAckActionStage`, `nativeRetryPublish`,
//! `nativeCancelPublish`.

use jni::objects::{JClass, JString};
use jni::sys::jlong;
use jni::JNIEnv;

use nmp_ffi::{nmp_app_ack_action_stage, nmp_app_cancel_action, nmp_app_retry_publish};

use crate::{jstring_to_cstring, session_arc};

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
