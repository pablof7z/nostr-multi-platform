//! Android JNI wrappers for the Rust-owned action seam.
//!
//! The bridge parses no action vocabulary. Kotlin supplies a namespace and
//! JSON body; Rust encodes the typed payload and dispatches through the byte
//! doorway (`nmp_app_dispatch_action_bytes`). Terminal stage cleanup goes back
//! through `nmp_app_ack_action_stage`.

use jni::objects::{JClass, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;

use nmp_app_chirp::{action_spec_for_intent_json, dispatch_action_bytes_for};
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

/// Convert raw Chirp user intent into a typed action and dispatch it through
/// the byte doorway in one JNI call (ADR-0064 / Cut-B host slice, #1782).
///
/// Kotlin passes user intent only (`intent_json`, a serialized
/// `ChirpActionIntent`). Rust owns the protocol body: it builds the canonical
/// `(namespace, body_json)` spec via `action_spec_for_intent_json` (NIP-10 reply
/// tags, kind:6 reposts, profile fields), then encodes the typed `ActionPayload`
/// bytes and dispatches them through the typed `nmp_app_dispatch_action_bytes`
/// doorway via `dispatch_action_bytes_for`. No JSON crosses the FFI to the
/// kernel.
///
/// Returns the Rust JSON envelope as a JNI string:
/// * `{"correlation_id":"<id>"}` — accepted and enqueued.
/// * `{"error":"<message>"}` — malformed intent / namespace, or rejected.
///
/// D6: null handle or malformed JNI arguments collapse to `"{}"`; Kotlin treats
/// that as a parse failure rather than pretending the action succeeded. A null
/// app inside the session yields the `"runtime app is not available"` error
/// envelope, never a crash.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeDispatchIntentBytes(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    intent_json: JString,
) -> jstring {
    let Some(s) = session_arc(handle) else {
        return json_string(env, "{}");
    };
    let Some(intent_json) = jstring_to_cstring(&mut env, &intent_json) else {
        return json_string(env, "{}");
    };
    let intent = intent_json.to_string_lossy();

    let result = match action_spec_for_intent_json(&intent) {
        Ok(spec) => match s
            .with_app(|app| dispatch_action_bytes_for(app, &spec.namespace, &spec.body_json))
        {
            Some(result) => result,
            None => return json_string(env, "{}"),
        },
        Err(error) => Err(error),
    };
    json_string(env, &dispatch_result_json(result))
}

/// Dispatch a pre-built Chirp action `(namespace, body_json)` through the byte
/// doorway (ADR-0064 / Cut-B host slice, #1782).
///
/// For the direct-JSON dispatch sites (wallet, relay-lists, NIP-29 group ops)
/// where Kotlin already holds a namespace + body and does NOT go through the
/// intent spec. Encodes the typed `ActionPayload` bytes via
/// `dispatch_action_bytes_for` and dispatches the typed bytes; no JSON crosses
/// the FFI to the kernel. Returns the same `{"correlation_id"}` / `{"error"}`
/// envelope as `nativeDispatchIntentBytes`.
///
/// D6: null handle or malformed JNI arguments collapse to `"{}"`; a null app
/// yields the `"runtime app is not available"` error envelope.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeDispatchActionBytes(
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
