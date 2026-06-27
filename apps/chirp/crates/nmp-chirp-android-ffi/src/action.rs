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
//! `nativeCancelPublish`, `nativeDispatchAction`.  These are residual JNI
//! symbols staged for a future migration lane.

use jni::objects::{JClass, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;

use nmp_app_chirp::dispatch_action_bytes_for;
use nmp_ffi::{nmp_app_ack_action_stage, nmp_app_cancel_action, nmp_app_retry_publish};
use serde_json::json;

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

/// Dispatch a named action through the typed byte doorway (ADR-0064 / Cut-B,
/// #1756).
///
/// Kotlin supplies `namespace` and `body_json` (the canonical serde action
/// body). Rust encodes the typed `ActionPayload` bytes via
/// `dispatch_action_bytes_for` and dispatches them through
/// `nmp_app_dispatch_action_bytes`. No JSON crosses the FFI to the kernel.
///
/// Returns the Rust JSON envelope as a JNI string:
/// * `{"correlation_id":"<id>"}` — accepted and enqueued.
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
    body_json: JString,
) -> jstring {
    let Some(s) = session_arc(handle) else {
        return json_string(env, "{}");
    };
    let Some(namespace) = jstring_to_cstring(&mut env, &namespace) else {
        return json_string(env, "{}");
    };
    let Some(body_json) = jstring_to_cstring(&mut env, &body_json) else {
        return json_string(env, "{}");
    };
    let namespace = namespace.to_string_lossy();
    let body_json = body_json.to_string_lossy();

    let Some(result) = s.with_app(|app| dispatch_action_bytes_for(app, &namespace, &body_json))
    else {
        return json_string(env, "{}");
    };
    json_string(env, &dispatch_result_json(result))
}

// nativeDispatchIntentBytes and nativeDispatchActionBytes were deleted in
// M14-0 (issue #2129).  Action dispatch for the app-loop lane is now served by
// `AppHandle::dispatch_action_json` / `dispatch_action_bytes` in
// `uniffi_app_loop.rs`; social writes ride the generated builder bytes (M14-1 /
// issue #2145).

/// Render a dispatch result as the canonical `{"correlation_id"}` /
/// `{"error"}` JSON envelope string. `serde_json::json!` keeps the message
/// escape-safe.
fn dispatch_result_json(result: Result<String, String>) -> String {
    match result {
        Ok(correlation_id) => json!({ "correlation_id": correlation_id }).to_string(),
        Err(error) => json!({ "error": error }).to_string(),
    }
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
