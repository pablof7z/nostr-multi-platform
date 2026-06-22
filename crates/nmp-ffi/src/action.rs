//! FFI action-dispatch entry point.
//!
//! [`nmp_app_dispatch_action`] is the single, namespace-keyed entry point
//! for the `substrate::ActionModule` family. Instead of one bespoke C symbol
//! per verb (`nmp_app_publish_note`, `nmp_app_react`, `nmp_app_follow`, …),
//! a caller names the action namespace and passes the action as JSON; the
//! [`nmp_core::__ffi_internal::ActionRegistry`] looks up the module and validates it.
//!
//! # Scope (M6 — execution wiring)
//!
//! This entry point performs **action validation, correlation-id assignment,
//! AND execution**. After [`nmp_core::__ffi_internal::ActionRegistry::start`] validates
//! the action and mints a correlation id, the dispatch path drives the
//! action through the actor:
//!
//! * For `nmp.publish` / [`PublishAction::Publish`], the validated signed
//!   event is converted to a [`nmp_store::RawEvent`] and handed to the
//!   actor via [`ActorCommand::PublishSignedEvent`] — the same actor command
//!   the (now-deleted) bespoke `nmp_app_publish_signed_event*` FFI symbols
//!   used to use, plus the workspace-internal
//!   [`crate::NmpApp::publish_signed_explicit`] Marmot seam. The actor
//!   re-verifies the Schnorr signature + id hash (D4 — only the actor loop
//!   signs/publishes; a forged event is rejected, never published) and
//!   routes it through the typed `PublishTarget` carried by the action.
//! * Publish *cancel* is NOT a dispatch action and NOT a `PublishAction`
//!   variant (the bespoke `PublishAction::Cancel` lane was deleted in S7,
//!   #1754). The publish lifecycle's control plane stays on the dedicated FFI
//!   symbols: `nmp_app_retry_publish` (by handle) and `nmp_app_cancel_action`
//!   (by operation `correlation_id` — the kernel reverse-resolves the publish
//!   handle so the `Cancelled` terminal lands under the original correlation_id,
//!   PD-036).
//!
//! A returned `{"correlation_id":"…"}` for a `Publish` action means the
//! event was *accepted and enqueued for publication* — the actor owns the
//! actual relay dispatch + ack tracking from there (the publish engine
//! reports per-relay outcomes through the normal snapshot path).
//!
//! # Threading
//!
//! The registry lives on [`NmpApp`], not on the actor-thread-owned
//! `Kernel` (`Kernel` is `!Send`). Registered modules are stateless ZST
//! adapters, so `start()` is a pure validator and is sound to call directly
//! on the FFI thread. Execution itself does NOT run on the FFI thread (D8 —
//! no blocking here): dispatch only *sends* an `ActorCommand` down the
//! existing channel; the actor loop signs/publishes (D4).
//!
//! # Doctrine
//!
//! * **D6** — nothing crosses this boundary as an exception. A null `app`,
//!   missing/invalid arguments, an unknown namespace, or malformed action
//!   JSON all come back as a populated `{"error":"…"}` JSON object. A
//!   non-null `app` never yields a NULL return.
//! * **D4** — the FFI thread never signs or publishes. It hands a
//!   pre-signed event to the actor; the actor verifies + publishes.
//! * **D8** — the FFI thread never blocks. Dispatch is a non-blocking
//!   channel send.

use std::ffi::{c_char, CString};

use super::{app_ref, c_string_argument, NmpApp};
use nmp_core::substrate::{ActionContext, ActionRejection, ActionResult};

// ADR-0064 / S4 (#1752) — the native byte doorway lives in this sibling so
// `action.rs` stays under its hand-authored LOC ceiling (AGENTS.md / V-12). It
// is a size-management seam, not an API boundary: `nmp_app_dispatch_action_bytes`
// is an ordinary `#[no_mangle]` C symbol and reaches the shared post-mint
// helpers (`finish_dispatch` / `error_json` / `rejection_message`) below.
mod bytes;

/// Dispatch a named action through the action registry.
///
/// Returns a freshly heap-allocated, NUL-terminated JSON C string the caller
/// MUST release via [`super::free::nmp_free_string`]
/// (`nmp_free_string`):
///
/// * `{"correlation_id":"<32-hex>"}` — the action was accepted, assigned a
///   correlation id, and (for `nmp.publish` `Publish`) enqueued with the
///   actor for execution. See the module docs for the per-namespace
///   execution contract.
/// * `{"error":"<message>"}` — the action was rejected (null app, invalid
///   arguments, unknown namespace, malformed/wrong-shape JSON).
///
/// D6: never returns NULL for a non-null `app`; every failure is data.
///
/// # Safety
/// `app` must be a valid non-null pointer from [`super::nmp_app_new`], or
/// null (a null `app` yields an error JSON, never a crash). `namespace` and
/// `action_json` must be valid UTF-8 NUL-terminated C strings, or null
/// (null/invalid are treated as empty and rejected).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_dispatch_action(
    app: *mut NmpApp,
    namespace: *const c_char,
    action_json: *const c_char,
) -> *mut c_char {
    let result = dispatch_action_json(
        app_ref(app),
        c_string_argument(namespace).as_deref().unwrap_or(""),
        c_string_argument(action_json).as_deref().unwrap_or(""),
    );
    // JSON never contains an interior NUL; the `c"{}"` literal fallback is
    // NUL-checked at compile time, so there is no runtime panic path (D6).
    CString::new(result)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

/// Host acknowledgement of a `correlation_id` in the `action_stages`
/// snapshot mirror.
///
/// The kernel projects `action_stages` (a `correlation_id → [StageEntry...]`
/// map) on every tick. Unlike `action_results` (drain on emit), the same
/// entry reappears every tick until the host calls this symbol. After the
/// host's UI has reacted to the terminal stage (`Accepted` / `Failed`) it
/// passes the `correlation_id` here to drop the entry from the projection.
///
/// `correlation_id` is the 32-hex operation-id the host received from
/// `nmp_app_dispatch_action`. A null `app`, a null/empty `correlation_id`, or
/// an unknown `correlation_id` is a silent no-op (D6 — never a crash).
///
/// THREADING: dispatch is non-blocking — this only enqueues
/// [`nmp_core::ActorCommand::AckActionStage`] on the actor channel
/// (D8 — no actor round-trip on the FFI thread). The kernel drops the entry
/// when the actor dequeues the command and the next snapshot tick emits
/// without it.
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
/// `correlation_id` must be a valid UTF-8 NUL-terminated C string (or null).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_ack_action_stage(app: *mut NmpApp, correlation_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(cid) = c_string_argument(correlation_id) else {
        return;
    };
    if cid.is_empty() {
        return;
    }
    app.send_cmd(nmp_core::ActorCommand::AckActionStage(cid));
}

/// Host-supplied action result observer callback.
///
/// Invoked after a dispatched action has been accepted by the registry and
/// enqueued for execution. Receives a NUL-terminated JSON C string
/// `{"correlation_id":"<hex>","result_json":<value>}` — the serialized
/// [`ActionResult`]. The pointer is owned by `nmp-core`, is valid only for
/// the duration of the callback, and MUST NOT be freed or retained by the
/// host; copy any needed bytes before returning.
///
/// This is an "action accepted" push signal, NOT a completion carrier.
/// Built-in executors are fire-and-forget, so `result_json` is `null`; an
/// action's eventual outcome is reported via the snapshot-projection (pull)
/// path, not this channel. See [`ActionResult`].
pub type NmpActionResultObserver = unsafe extern "C" fn(*const c_char);

/// Register a host-supplied action-result observer against the app's action
/// registry — the *push* counterpart to the snapshot-projection (pull)
/// output seam.
///
/// After [`nmp_app_dispatch_action`] validates an action and its executor
/// returns `Ok`, the registry hands the observer a JSON string
/// `{"correlation_id":"<hex>","result_json":<value>}`. For built-in
/// (fire-and-forget) executors `result_json` is `null`; the signal means the
/// action was *accepted and enqueued*, not that the actor has finished
/// publishing.
///
/// THREADING: this call takes `&NmpApp` (the observer lives behind an
/// `Arc<Mutex<…>>` slot), so — unlike the typed `register_action::<M>()`
/// Rust seam — it may be invoked before *or after* `nmp_app_start`. A second
/// registration replaces the first.
///
/// A null `app` or a null `observer` is a silent no-op (D6: a bad
/// registration argument never crashes the host).
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
/// `observer`, when `Some`, must be a valid function pointer for the
/// remaining lifetime of `app` — the registry retains it.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_register_action_result_observer(
    app: *mut NmpApp,
    observer: Option<NmpActionResultObserver>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(observer) = observer else {
        return;
    };
    app.register_action_result_observer(move |result: ActionResult| {
        // Serialize the `ActionResult` to its `{"correlation_id":…,
        // "result_json":…}` JSON shape. `serde_json` output never contains
        // an interior NUL, so `CString::new` does not fail in practice; a
        // failure is treated as a silent drop (D6 — never panic across the
        // ABI boundary).
        let Ok(json) = serde_json::to_string(&result) else {
            return;
        };
        let Ok(cstr) = CString::new(json) else {
            return;
        };
        // SAFETY: `observer` is a valid function pointer per this symbol's
        // safety contract; `cstr.as_ptr()` is a valid NUL-terminated C
        // string live for the duration of the call.
        //
        // D6 — wrap the foreign callback in `guard_ffi_callback` for the
        // same reason the kernel-event / raw-event observer fan-outs do:
        // a Swift `fatalError` / Kotlin exception thrown from the host's
        // observer would otherwise unwind across the C ABI (undefined
        // behaviour). The outer `deliver_result` also wraps its closure
        // in `catch_unwind`, so a Rust panic raised by serde / `CString`
        // is already contained; this guard closes the foreign-throw half
        // of the gap.
        let _: Option<()> =
            nmp_core::ffi_guard::guard_ffi_callback("action result observer", || unsafe {
                observer(cstr.as_ptr());
            });
    });
}

/// Pure (FFI-free) core of [`nmp_app_dispatch_action`]: validate the action
/// against the registry, drive its execution through the actor, and return
/// the JSON result string. Split out so the unit tests can exercise the
/// dispatch logic without raw pointers.
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
    let mut ctx = ActionContext {};
    match app
        .action_registry
        .start(&mut ctx, dispatch_now_ms, namespace, action_json)
    {
        Ok(correlation_id) => {
            // `start()` validated the action and minted the correlation id.
            // Drive execution and turn the outcome into the JSON result. The
            // execute closure is the JSON-twin (`execute_action`); the
            // post-mint outcome handling is shared with the byte doorway via
            // `finish_dispatch` so both paths report acceptance / failure
            // identically (#1676 BUG-A/B/C).
            finish_dispatch(app, &correlation_id, execute_action(app, namespace, action_json, &correlation_id))
        }
        Err(rejection) => rejection_json(rejection),
    }
}

/// Shared post-mint outcome handling for BOTH the JSON ([`dispatch_action_json`])
/// and byte ([`dispatch_action_bytes`]) doorways.
///
/// `start()`/`start_bytes()` already minted `correlation_id`; this turns the
/// execute outcome into the JSON result and preserves the one-terminal-per-
/// dispatch invariant (#1676 BUG-A/B/C):
///
/// * `Ok(())` — accepted + enqueued; deliver the "accepted" observer signal,
///   return `{"correlation_id":…}`.
/// * `Err(failure)` with `enqueued` — the executor already enqueued an
///   `ActorCommand` and then failed; that command OWNS the terminal verdict, so
///   do NOT fan a second `RecordActionFailure` (double-terminal). Report as
///   accepted.
/// * `Err(failure)` without `enqueued` — nothing was enqueued; this fan-in is
///   the SOLE terminal. Record a `Failed { reason }` so the host spinner keyed
///   on the id resolves, and return BOTH id and error so the host can ACK +
///   toast.
fn finish_dispatch(
    app: &NmpApp,
    correlation_id: &str,
    outcome: Result<(), nmp_core::__ffi_internal::ActionExecuteFailure>,
) -> String {
    match outcome {
        Ok(()) => {
            app.action_registry.deliver_result(ActionResult {
                correlation_id: correlation_id.to_string(),
                result_json: serde_json::Value::Null,
            });
            format!(r#"{{"correlation_id":{}}}"#, json_string(correlation_id))
        }
        Err(failure) if failure.enqueued => {
            app.action_registry.deliver_result(ActionResult {
                correlation_id: correlation_id.to_string(),
                result_json: serde_json::Value::Null,
            });
            format!(r#"{{"correlation_id":{}}}"#, json_string(correlation_id))
        }
        Err(failure) => {
            app.send_cmd(nmp_core::ActorCommand::RecordActionFailure {
                correlation_id: correlation_id.to_string(),
                reason: failure.message.clone(),
            });
            error_json_with_correlation_id(correlation_id, &failure.message)
        }
    }
}

/// Drive the validated action toward execution via the registry's executor
/// map. Each module registers its own executor in
/// [`nmp_core::__ffi_internal::default_registry`]; this function delegates without
/// naming any module directly (D0).
///
/// `correlation_id` is the registry-minted action id the caller will return
/// to the host. It is forwarded to the executor so an `ActorCommand` whose
/// terminal verdict must carry this id (the `PublishRaw` path) can be built
/// with it.
fn execute_action(
    app: &NmpApp,
    namespace: &str,
    action_json: &str,
    correlation_id: &str,
) -> Result<(), nmp_core::__ffi_internal::ActionExecuteFailure> {
    app.action_registry
        .execute(namespace, action_json, correlation_id, &|cmd| {
            app.send_cmd(cmd);
        })
}

/// Flatten an [`ActionRejection`] into a human-readable message.
///
/// For [`ActionRejection::InvalidCoded`] callers that need the machine code,
/// use [`rejection_json`] directly instead.
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
/// lifecycle (`nmp_app_ack_action_stage`) once the next snapshot carries
/// the `action_stages` entry. Both fields are JSON-escaped via
/// [`json_string`].
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
#[path = "action/tests_host_op.rs"]
mod tests_host_op;

#[cfg(test)]
#[path = "action/terminal_correctness_tests.rs"]
mod terminal_correctness_tests;

#[cfg(test)]
#[path = "action/coded_rejection_tests.rs"]
mod coded_rejection_tests;
