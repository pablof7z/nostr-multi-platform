//! FFI action-dispatch entry points.
//!
//! The remaining test seam accepts a host-minted `correlation_id`, the
//! action's HOST namespace, and a typed
//! [`ActionPayload`](nmp_core::substrate::ActionPayload) FlatBuffers payload
//! wrapped in an open
//! [`DispatchEnvelope`](nmp_core::dispatch_envelope). The former JSON doorway
//! (`nmp_app_dispatch_action`) has been deleted; all callers have been migrated
//! to the typed byte path.
//!
//! # Scope (M6 — execution wiring)
//!
//! This entry point performs **action validation, correlation-id assignment,
//! AND execution**. After [`nmp_core::__ffi_internal::ActionRegistry::start_bytes`]
//! validates the action and records the host-supplied correlation id, the
//! dispatch path drives the action through the actor:
//!
//! * For `nmp.publish`, app-facing actions are unsigned write intents such as
//!   `PublishRaw`, `PublishProfile`, and `PublishReply`. The actor finalizes,
//!   signs, routes, and publishes them. Pre-signed publish is intentionally not
//!   accepted on this byte doorway; verbatim/imported/protocol-owned signed
//!   events use internal seams such as [`crate::NmpApp::publish_signed_explicit`]
//!   and must carry explicit provenance before they reach the actor.
//! * Publish *cancel* is NOT a dispatch action and NOT a `PublishAction`
//!   variant (the bespoke `PublishAction::Cancel` lane was deleted in S7,
//!   #1754). Publish lifecycle control is handled by the native runtime and
//!   UniFFI surfaces, not this C ABI crate.
//!
//! A returned `{"correlation_id":"…"}` for a publish action means the write
//! intent was *accepted and enqueued for publication* — the actor owns signing,
//! relay dispatch, and ack tracking from there (the publish engine reports
//! per-relay outcomes through the normal snapshot path).
//!
//! # Threading
//!
//! The registry lives on [`NmpApp`], not on the actor-thread-owned
//! `Kernel` (`Kernel` is `!Send`). Registered modules are stateless ZST
//! adapters, so `start_bytes()` is a pure validator and is sound to call
//! directly on the FFI thread. Execution itself does NOT run on the FFI
//! thread (D8 — no blocking here): dispatch only *sends* an `ActorCommand`
//! down the existing channel; the actor loop signs/publishes (D4).
//!
//! # Doctrine
//!
//! * **D6** — nothing crosses this boundary as an exception. A null `app`,
//!   missing/invalid arguments, an unknown namespace, or a malformed payload
//!   all come back as a populated `{"error":"…"}` JSON object. A non-null
//!   `app` never yields a NULL return.
//! * **D4** — the FFI thread never signs or publishes. It hands unsigned app
//!   intent to the actor; the actor finalizes, signs, verifies, and publishes.
//! * **D8** — the FFI thread never blocks. Dispatch is a non-blocking
//!   channel send.

#[cfg(any(test, feature = "test-support"))]
use super::NmpApp;
#[cfg(any(test, feature = "test-support"))]
use nmp_core::actor::ActionLedgerCommand;
#[cfg(any(test, feature = "test-support"))]
use nmp_core::substrate::{ActionContext, ActionRejection, ActionResult};

// ADR-0064 / S4 (#1752) — the byte-dispatch JSON serialization test seam lives
// in this sibling so `action.rs` stays under its hand-authored LOC ceiling
// (AGENTS.md / V-12). The dispatch core lives in
// `nmp_native_runtime::action_dispatch`.
mod bytes;

/// Pure (FFI-free) core of the action dispatch logic: validate the action
/// against the registry, drive its execution through the actor, and return
/// the JSON result string. Used by nmp-ffi action tests that exercise the
/// JSON dispatch logic without going through the byte doorway.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn dispatch_action_json(
    app: Option<&NmpApp>,
    namespace: &str,
    action_json: &str,
) -> String {
    let Some(app) = app else {
        return error_json("null app");
    };
    let dispatch_now_ms = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };
    let mut ctx = ActionContext::with_event_store_slot(app.event_store_handle());
    match app
        .action_registry()
        .start(&mut ctx, dispatch_now_ms, namespace, action_json)
    {
        Ok(correlation_id) => finish_dispatch(
            app,
            &correlation_id,
            execute_action(app, &ctx, namespace, action_json, &correlation_id),
        ),
        Err(rejection) => rejection_json(rejection),
    }
}

/// Post-mint outcome handling for the test-only JSON doorway
/// ([`dispatch_action_json`]).
///
/// `start()` already minted `correlation_id`; this turns the execute outcome
/// into the JSON result and preserves the one-terminal-per-dispatch invariant
/// (#1676 BUG-A/B/C). The byte doorway uses the typed equivalent in
/// `nmp_native_runtime::action_dispatch::finish_dispatch_typed` instead.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn finish_dispatch(
    app: &NmpApp,
    correlation_id: &str,
    outcome: Result<(), nmp_core::__ffi_internal::ActionExecuteFailure>,
) -> String {
    match outcome {
        Ok(()) => {
            app.action_registry().deliver_result(ActionResult {
                correlation_id: correlation_id.to_string(),
                result_json: serde_json::Value::Null,
            });
            format!(r#"{{"correlation_id":{}}}"#, json_string(correlation_id))
        }
        Err(failure) if failure.enqueued => {
            app.action_registry().deliver_result(ActionResult {
                correlation_id: correlation_id.to_string(),
                result_json: serde_json::Value::Null,
            });
            format!(r#"{{"correlation_id":{}}}"#, json_string(correlation_id))
        }
        Err(failure) => {
            app.send_cmd(nmp_core::actor::ActorCommand::ActionLedger(
                ActionLedgerCommand::RecordFailure {
                    correlation_id: correlation_id.to_string(),
                    reason: failure.message.clone(),
                },
            ));
            error_json_with_correlation_id(correlation_id, &failure.message)
        }
    }
}

/// Drive the validated action toward execution via the registry's executor map.
#[cfg(any(test, feature = "test-support"))]
fn execute_action(
    app: &NmpApp,
    ctx: &ActionContext,
    namespace: &str,
    action_json: &str,
    correlation_id: &str,
) -> Result<(), nmp_core::__ffi_internal::ActionExecuteFailure> {
    app.action_registry()
        .execute(ctx, namespace, action_json, correlation_id, &|cmd| {
            app.send_cmd(cmd);
        })
}

/// Flatten an [`ActionRejection`] into a human-readable message.
///
/// For [`ActionRejection::InvalidCoded`] callers that need the machine code,
/// use [`rejection_json`] directly instead.
#[cfg(any(test, feature = "test-support"))]
fn rejection_message(rejection: ActionRejection) -> String {
    match rejection {
        ActionRejection::Invalid(s) => s,
        ActionRejection::InvalidCoded { message, .. } => message,
        ActionRejection::Unauthorized(s) => format!("unauthorized: {s}"),
        ActionRejection::Conflict(s) => format!("conflict: {s}"),
    }
}

/// Build a `{"error":"…"}` or `{"error":"…","code":"…"}` JSON object from an
/// [`ActionRejection`].
///
/// [`ActionRejection::InvalidCoded`] carries a stable machine `code` that
/// shells use to localize the error (issue #1734). All other variants produce
/// the plain `{"error":"…"}` envelope (no `code` field).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn rejection_json(rejection: ActionRejection) -> String {
    match rejection {
        ActionRejection::InvalidCoded { code, message } => {
            format!(
                r#"{{"error":{},"code":{}}}"#,
                json_string(&message),
                json_string(code),
            )
        }
        other => error_json(&rejection_message(other)),
    }
}

/// Build an `{"error":"…"}` JSON object with `msg` JSON-escaped.
fn error_json(msg: &str) -> String {
    format!(r#"{{"error":{}}}"#, json_string(msg))
}

/// `{"correlation_id":"…","error":"…"}` envelope for the post-mint
/// failure path. The `correlation_id` was already minted by
/// [`ActionRegistry::start`] and a `Failed` terminal stage has been queued
/// to the actor; including the id here lets the host drive the ACK
/// lifecycle once the next snapshot carries the `action_stages` entry. Both
/// fields are JSON-escaped via
/// [`json_string`].
#[cfg(any(test, feature = "test-support"))]
fn error_json_with_correlation_id(correlation_id: &str, msg: &str) -> String {
    format!(
        r#"{{"correlation_id":{},"error":{}}}"#,
        json_string(correlation_id),
        json_string(msg)
    )
}

/// JSON-encode a string (quotes + escaping). Falls back to `""` — an empty
/// JSON string — if encoding somehow fails, so the surrounding object stays
/// well-formed (D6: failures are data, never panics).
fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

#[cfg(test)]
#[path = "action/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "action/host_registration_tests.rs"]
mod host_registration_tests;

#[cfg(test)]
#[path = "action/tests_host_op.rs"]
mod tests_host_op;

#[cfg(test)]
#[path = "action/terminal_correctness_tests.rs"]
mod terminal_correctness_tests;

#[cfg(test)]
#[path = "action/coded_rejection_tests.rs"]
mod coded_rejection_tests;

#[cfg(test)]
#[path = "action/s10_gates_tests.rs"]
mod s10_gates_tests;
