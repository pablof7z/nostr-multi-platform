//! FFI action-dispatch entry point.
//!
//! [`nmp_app_dispatch_action`] is the single, namespace-keyed entry point
//! for the `substrate::ActionModule` family. Instead of one bespoke C symbol
//! per verb (`nmp_app_publish_note`, `nmp_app_react`, `nmp_app_follow`, …),
//! a caller names the action namespace and passes the action as JSON; the
//! [`crate::kernel::ActionRegistry`] looks up the module and validates it.
//!
//! # Scope (M6 — execution wiring)
//!
//! This entry point performs **action validation, correlation-id assignment,
//! AND execution**. After [`crate::kernel::ActionRegistry::start`] validates
//! the action and mints a correlation id, the dispatch path drives the
//! action through the actor:
//!
//! * For `nmp.publish` / [`PublishAction::Publish`], the validated signed
//!   event is converted to a [`crate::store::RawEvent`] and handed to the
//!   actor via [`ActorCommand::PublishSignedEvent`] — the same actor command
//!   the `nmp_app_publish_signed_event*` symbols already use. The actor
//!   re-verifies the Schnorr signature + id hash (D4 — only the actor loop
//!   signs/publishes; a forged event is rejected, never published) and
//!   routes it through the NIP-65 outbox resolver.
//! * For [`PublishAction::Cancel`], no actor command exists yet — the
//!   registry already reports `ActionStatus::Cancelled`, so dispatch returns
//!   the correlation id without an actor round-trip. Wiring a real cancel
//!   into the publish engine is a follow-up.
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

use std::ffi::{c_char, CStr, CString};

use super::{app_ref, app_ref_mut, c_string_argument, NmpApp};
use crate::substrate::{ActionContext, ActionRejection};

/// Dispatch a named action through the action registry.
///
/// Returns a freshly heap-allocated, NUL-terminated JSON C string the caller
/// MUST release via [`super::capability::nmp_app_free_string`]
/// (`nmp_app_free_string`):
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
        &c_str_lossy(namespace),
        &c_str_lossy(action_json),
    );
    // JSON never contains an interior NUL; the `c"{}"` literal fallback is
    // NUL-checked at compile time, so there is no runtime panic path (D6).
    CString::new(result)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

/// Host-supplied action executor callback.
///
/// Receives the already-validated `action_json` as a NUL-terminated C string.
/// Returns `NULL` on success, or a NUL-terminated C string describing the
/// failure. The returned error string is read immediately and copied into an
/// owned Rust `String`; the host owns its lifetime and may free or reuse it
/// after the callback returns.
pub type NmpActionExecutor = unsafe extern "C" fn(*const c_char) -> *const c_char;

/// Register a host-supplied executor for `namespace` against the app's
/// action registry — the post-construction registration seam.
///
/// This is the C-ABI counterpart to [`NmpApp::register_action_executor`]: a
/// host can wire an action namespace into the registry **without editing
/// `nmp-core`**. The bridge closure copies the action JSON into a
/// NUL-terminated C string, invokes `executor`, and maps its return value
/// (`NULL` → `Ok(())`, non-NULL → `Err(message)`).
///
/// SCOPE (v1): this exposes *executor* registration only. The full
/// [`nmp_app_dispatch_action`] path also requires a registered *module*
/// (`ActionRegistry::start` validates the JSON shape against it), so a
/// namespace wired through this symbol alone is reachable by the registry's
/// internal `execute` path but not yet by `nmp_app_dispatch_action`. Module
/// registration is the planned follow-up (`nmp_app_register_action_module`).
///
/// THREADING: this call takes `&mut NmpApp`. It MUST be invoked during host
/// init — before `nmp_app_start` and before any `nmp_app_dispatch_action` —
/// so no shared `&NmpApp` is live on another thread. See [`app_ref_mut`].
///
/// A null `app`, a null/empty/invalid `namespace`, or a null `executor` is a
/// silent no-op (D6: a bad registration argument never crashes the host).
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
/// `namespace` must be a valid UTF-8 NUL-terminated C string (or null).
/// `executor`, when `Some`, must be a valid function pointer for the
/// remaining lifetime of `app` — the registry retains it.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_register_action_executor(
    app: *mut NmpApp,
    namespace: *const c_char,
    executor: Option<NmpActionExecutor>,
) {
    let Some(app) = app_ref_mut(app) else {
        return;
    };
    let Some(ns) = c_string_argument(namespace) else {
        return;
    };
    let Some(exec) = executor else {
        return;
    };
    // Leak the namespace so it lives `'static` — the registry keys on
    // `&'static str`. This is a registration-time-only call (host init), so
    // the leak is bounded by the number of registered namespaces, not by
    // runtime activity.
    let ns: &'static str = Box::leak(ns.into_boxed_str());
    app.register_action_executor(ns, move |action_json, _send| {
        // The host executor speaks JSON only. The `_send` actor-command
        // bridge is intentionally unused in v1: a host executor that needs
        // to enqueue an `ActorCommand` uses a separate mechanism; the seam
        // here proves post-construction registration works.
        let cstr = CString::new(action_json).map_err(|e| e.to_string())?;
        // SAFETY: `exec` is a valid function pointer per this symbol's
        // safety contract; `cstr.as_ptr()` is a valid NUL-terminated C
        // string live for the duration of the call.
        let result_ptr = unsafe { exec(cstr.as_ptr()) };
        if result_ptr.is_null() {
            Ok(())
        } else {
            // SAFETY: a non-null return is, per the callback contract, a
            // valid NUL-terminated C string. Copied immediately into an
            // owned `String`; the host retains ownership of the pointer.
            let msg = unsafe { CStr::from_ptr(result_ptr) }
                .to_string_lossy()
                .into_owned();
            Err(msg)
        }
    });
}

/// Host-supplied action *validator* callback.
///
/// Receives the raw `action_json` as a NUL-terminated C string. Returns
/// `NULL` to **accept** the action (the registry mints a correlation id with
/// a default `Pending` plan), or a NUL-terminated C string describing the
/// **rejection** reason. The returned error string is read immediately and
/// copied into an owned Rust `String`; the host owns its lifetime and may
/// free or reuse it after the callback returns.
pub type NmpActionValidator = unsafe extern "C" fn(*const c_char) -> *const c_char;

/// Register a host-supplied *module validator* for `namespace` against the
/// app's action registry — the complement to
/// [`nmp_app_register_action_executor`].
///
/// `nmp_app_dispatch_action` has two phases: `start()` validates the action
/// JSON against a registered **module**, then `execute()` dispatches it via a
/// registered **executor**. `nmp_app_register_action_executor` wires the
/// executor half; this symbol wires the module half. Registering **both** for
/// a namespace makes it fully reachable through `nmp_app_dispatch_action`
/// **without editing `nmp-core`** — a host can dispatch any custom namespace.
///
/// The `validator` callback receives the action JSON and returns `NULL` to
/// accept (the action gets a default `Pending` plan) or a NUL-terminated
/// error string to reject. Passing a `NULL` `validator` registers an
/// **accept-all** module: every action under `namespace` is accepted with a
/// default `Pending` plan — useful when shape validation lives entirely in
/// the host's executor.
///
/// THREADING: this call takes `&mut NmpApp`. It MUST be invoked during host
/// init — before `nmp_app_start` and before any `nmp_app_dispatch_action` —
/// so no shared `&NmpApp` is live on another thread. See [`app_ref_mut`].
///
/// A null `app` or a null/empty/invalid `namespace` is a silent no-op (D6: a
/// bad registration argument never crashes the host). A null `validator` is
/// NOT a no-op — it deliberately selects the accept-all module above.
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
/// `namespace` must be a valid UTF-8 NUL-terminated C string (or null).
/// `validator`, when `Some`, must be a valid function pointer for the
/// remaining lifetime of `app` — the registry retains it.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_register_action_module(
    app: *mut NmpApp,
    namespace: *const c_char,
    validator: Option<NmpActionValidator>,
) {
    let Some(app) = app_ref_mut(app) else {
        return;
    };
    let Some(ns) = c_string_argument(namespace) else {
        return;
    };
    // Leak the namespace so it lives `'static` — the registry keys on
    // `&'static str`. Registration-time-only call (host init), so the leak is
    // bounded by the number of registered namespaces, not by runtime activity.
    let ns: &'static str = Box::leak(ns.into_boxed_str());
    let Some(validate) = validator else {
        // No validator → accept-all: every action is accepted with a default
        // pending plan. Shape validation is then the host executor's job.
        app.register_action_module(ns, |_action_json| Ok(default_pending_plan()));
        return;
    };
    app.register_action_module(ns, move |action_json| {
        use crate::substrate::ActionRejection;
        // The host validator speaks JSON only. An interior NUL in the action
        // JSON cannot cross to C — surface it as a rejection (D6: failures
        // are data, never a panic).
        let cstr = CString::new(action_json)
            .map_err(|_| ActionRejection::Invalid("action_json contains NUL byte".into()))?;
        // SAFETY: `validate` is a valid function pointer per this symbol's
        // safety contract; `cstr.as_ptr()` is a valid NUL-terminated C string
        // live for the duration of the call.
        let result_ptr = unsafe { validate(cstr.as_ptr()) };
        if result_ptr.is_null() {
            // NULL return = accept with the default pending plan.
            Ok(default_pending_plan())
        } else {
            // SAFETY: a non-null return is, per the callback contract, a
            // valid NUL-terminated C string. Copied immediately into an owned
            // `String`; the host retains ownership of the pointer.
            let msg = unsafe { CStr::from_ptr(result_ptr) }
                .to_string_lossy()
                .into_owned();
            Err(ActionRejection::Invalid(msg))
        }
    });
}

/// The default erased [`ActionPlan`] a host-registered module hands back on
/// **accept**: a `Pending` action with a `"Pending"` step and no deadline.
/// The host's executor owns the real work; the plan is only the registry's
/// initial-status bookkeeping (the M6 action ledger may persist it later).
fn default_pending_plan() -> crate::substrate::ActionPlan<serde_json::Value> {
    use crate::substrate::{ActionPlan, ActionStatus};
    ActionPlan {
        initial_step: serde_json::Value::String("Pending".into()),
        initial_status: ActionStatus::Pending,
        deadline_ms: None,
    }
}

/// Pure (FFI-free) core of [`nmp_app_dispatch_action`]: validate the action
/// against the registry, drive its execution through the actor, and return
/// the JSON result string. Split out so the unit tests can exercise the
/// dispatch logic without raw pointers.
fn dispatch_action_json(app: Option<&NmpApp>, namespace: &str, action_json: &str) -> String {
    let Some(app) = app else {
        return error_json("null app");
    };
    let mut ctx = ActionContext {
        now_ms: now_ms(),
    };
    match app.action_registry.start(&mut ctx, namespace, action_json) {
        Ok((correlation_id, _plan)) => {
            // `_plan` (the `ActionPlan`) is intentionally dropped: plan
            // persistence is the M6 action ledger's job (a follow-up). The
            // correlation id is the handle the caller acts on.
            //
            // Execution: `start()` only validated the action. Now drive it
            // through the actor. `execute_action` is namespace-aware; an
            // execution failure is surfaced as `{"error":...}` (D6) and the
            // already-minted correlation id is discarded — a rejected
            // dispatch must not look like an accepted one.
            match execute_action(app, namespace, action_json) {
                Ok(()) => format!(
                    r#"{{"correlation_id":{}}}"#,
                    json_string(&correlation_id)
                ),
                Err(msg) => error_json(&msg),
            }
        }
        Err(rejection) => error_json(&rejection_message(rejection)),
    }
}

/// Drive the validated action toward execution via the registry's executor
/// map. Each module registers its own executor in
/// [`crate::kernel::default_registry`]; this function delegates without
/// naming any module directly (D0).
fn execute_action(app: &NmpApp, namespace: &str, action_json: &str) -> Result<(), String> {
    app.action_registry
        .execute(namespace, action_json, &|cmd| app.send_cmd(cmd))
}

/// Flatten an [`ActionRejection`] into a human-readable message.
fn rejection_message(rejection: ActionRejection) -> String {
    match rejection {
        ActionRejection::Invalid(s) => s,
        ActionRejection::Unauthorized(s) => format!("unauthorized: {s}"),
        ActionRejection::Conflict(s) => format!("conflict: {s}"),
    }
}

/// Build an `{"error":"…"}` JSON object with `msg` JSON-escaped.
fn error_json(msg: &str) -> String {
    format!(r#"{{"error":{}}}"#, json_string(msg))
}

/// JSON-encode a string (quotes + escaping). Falls back to `""` — an empty
/// JSON string — if encoding somehow fails, so the surrounding object stays
/// well-formed (D6: failures are data, never panics).
fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Current wall-clock time in milliseconds since the Unix epoch, for
/// [`ActionContext::now_ms`]. A clock before the epoch collapses to `0`.
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Decode a C string argument to an owned `String`. Null or invalid UTF-8
/// collapses to an empty string — the registry then rejects it as data.
fn c_str_lossy(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: caller guarantees a non-null `ptr` is a valid NUL-terminated
    // C string for the duration of this call.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{nmp_app_free, nmp_app_new};

    /// Run `body` against a fresh `NmpApp`, freeing it afterwards. The raw
    /// pointer from `nmp_app_new` is non-null and valid for the closure's
    /// lifetime; `nmp_app_free` reclaims it (its `Drop` joins the actor).
    fn with_app(body: impl FnOnce(&NmpApp)) {
        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null; the pointer is valid
        // until `nmp_app_free` below.
        body(unsafe { &*app });
        nmp_app_free(app);
    }

    /// The verification case from the task: dispatching a publish action
    /// returns a `correlation_id` string. `PublishAction::Cancel` is used
    /// because it only needs a non-empty handle — no signed-event fixture —
    /// and still exercises the full registry → adapter → module path.
    #[test]
    fn dispatch_cancel_action_returns_correlation_id() {
        with_app(|app| {
            let out = dispatch_action_json(
                Some(app),
                "nmp.publish",
                r#"{"Cancel":{"handle":"smoke-test"}}"#,
            );
            let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
            let id = parsed
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .expect("expected a correlation_id field");
            assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
            assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        });
    }

    #[test]
    fn dispatch_unknown_namespace_returns_error_json() {
        with_app(|app| {
            let out = dispatch_action_json(Some(app), "nmp.unknown", "{}");
            let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
            let err = parsed.get("error").and_then(|v| v.as_str()).unwrap();
            assert!(err.contains("unknown action namespace"), "got: {err}");
        });
    }

    #[test]
    fn dispatch_malformed_json_returns_error_json() {
        with_app(|app| {
            let out = dispatch_action_json(Some(app), "nmp.publish", "{bad json");
            let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
            assert!(parsed.get("error").is_some(), "expected error object: {out}");
        });
    }

    #[test]
    fn dispatch_null_app_returns_error_json() {
        let out = dispatch_action_json(None, "nmp.publish", "{}");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed.get("error").and_then(|v| v.as_str()),
            Some("null app")
        );
    }

    use crate::publish::{PublishAction, PublishTarget};
    use crate::substrate::{SignedEvent, UnsignedEvent};

    fn fixture_signed_event() -> SignedEvent {
        SignedEvent {
            id: "a".repeat(64),
            sig: "b".repeat(128),
            unsigned: UnsignedEvent {
                pubkey: "c".repeat(64),
                kind: 1,
                tags: vec![vec!["t".to_string(), "nmp".to_string()]],
                content: "hello from dispatch_action".to_string(),
                created_at: 1_700_000_000,
            },
        }
    }

    /// A `Publish` action through `dispatch_action` returns a correlation id.
    /// The executor is now registered in the registry (not hardcoded here),
    /// so this verifies the full registry → executor → actor-channel path.
    #[test]
    fn dispatch_publish_action_returns_correlation_id() {
        with_app(|app| {
            let action = PublishAction::Publish {
                handle: "h1".to_string(),
                event: fixture_signed_event(),
                target: PublishTarget::Auto,
            };
            let action_json = serde_json::to_string(&action).unwrap();
            let out = dispatch_action_json(Some(app), "nmp.publish", &action_json);
            let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
            let id = parsed
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("expected correlation_id, got: {out}"));
            assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
        });
    }

    #[test]
    fn execute_action_publish_is_ok() {
        with_app(|app| {
            let action = PublishAction::Publish {
                handle: "h2".to_string(),
                event: fixture_signed_event(),
                target: PublishTarget::Explicit {
                    relays: vec!["wss://relay.example".to_string()],
                },
            };
            let action_json = serde_json::to_string(&action).unwrap();
            assert!(
                execute_action(app, "nmp.publish", &action_json).is_ok(),
                "publish execution should not error"
            );
        });
    }

    #[test]
    fn execute_action_cancel_is_ok_without_actor() {
        with_app(|app| {
            let json = r#"{"Cancel":{"handle":"h3"}}"#;
            assert!(execute_action(app, "nmp.publish", json).is_ok());
        });
    }

    /// An unrecognized namespace has no executor — `execute_action` returns
    /// `Err` (D6), so a host is never handed a correlation id for an action
    /// that was silently dropped.
    #[test]
    fn execute_action_unknown_namespace_returns_err() {
        with_app(|app| {
            let err = execute_action(app, "nmp.future", "{}")
                .expect_err("unwired namespace must surface an error");
            assert!(
                err.contains("no executor registered") && err.contains("nmp.future"),
                "error should name the unwired namespace, got: {err}"
            );
        });
    }

    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    /// THE SEAM PROOF (review #14): a host registers an action executor for a
    /// namespace `nmp-core` has never heard of (`test.greeting`) *after*
    /// `NmpApp` construction, and dispatching that namespace runs the
    /// host-supplied closure. This is the post-construction registration the
    /// project lacked: no edit to `default_registry()`, no per-verb C symbol.
    #[test]
    fn host_registered_executor_dispatches_successfully() {
        let called = Arc::new(AtomicBool::new(false));
        let called_in_closure = Arc::clone(&called);

        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null; the pointer is valid
        // until `nmp_app_free` below, and no other reference aliases it here.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action_executor("test.greeting", move |action_json, _send| {
            assert_eq!(action_json, r#"{"hello":"world"}"#);
            called_in_closure.store(true, Ordering::SeqCst);
            Ok(())
        });

        // `test_execute_action` drives the registry's `execute` path
        // directly — the v1 seam exposes executor (not module) registration,
        // so `nmp_app_dispatch_action`'s `start()` validation is bypassed.
        app_mut
            .test_execute_action("test.greeting", r#"{"hello":"world"}"#)
            .expect("host-registered executor should run");

        assert!(
            called.load(Ordering::SeqCst),
            "host-registered executor was never invoked"
        );
        nmp_app_free(app);
    }

    /// A host executor that returns `Err` propagates the failure message
    /// back through the registry — the host is never handed a false success.
    #[test]
    fn host_registered_executor_propagates_error() {
        let app = nmp_app_new();
        // SAFETY: see `host_registered_executor_dispatches_successfully`.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action_executor("test.failing", |_action_json, _send| {
            Err("host rejected the action".to_string())
        });

        let err = app_mut
            .test_execute_action("test.failing", "{}")
            .expect_err("a failing host executor must surface an error");
        assert_eq!(err, "host rejected the action");
        nmp_app_free(app);
    }

    /// A namespace with no registered executor still returns the registry's
    /// `Err` — registering one namespace does not accidentally answer for
    /// another (D6: a missing executor is never silently swallowed).
    #[test]
    fn unregistered_namespace_after_host_registration_still_errs() {
        let app = nmp_app_new();
        // SAFETY: see `host_registered_executor_dispatches_successfully`.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action_executor("test.greeting", |_json, _send| Ok(()));

        let err = app_mut
            .test_execute_action("test.unregistered", "{}")
            .expect_err("an unregistered namespace must still error");
        assert!(
            err.contains("no executor registered") && err.contains("test.unregistered"),
            "error should name the unregistered namespace, got: {err}"
        );
        nmp_app_free(app);
    }

    /// THE SEAM PROOF: a host registers BOTH a module validator and an
    /// executor for a namespace `nmp-core` has never heard of (`test.todo`)
    /// *after* `NmpApp` construction, and `nmp_app_dispatch_action` then
    /// drives that namespace end-to-end — `start()` validation succeeds
    /// against the host module, `execute()` runs the host executor, and a
    /// `correlation_id` comes back. This is what PR #60 alone could NOT do:
    /// an executor-only namespace is rejected by `start()`. Together the two
    /// PRs give a host complete post-construction `dispatch_action` wiring.
    #[test]
    fn host_registered_module_and_executor_enables_dispatch_action() {
        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null; the pointer is valid
        // until `nmp_app_free` below, and no other reference aliases it here.
        let app_mut = unsafe { &mut *app };
        // Register both halves for "test.todo".
        app_mut.register_action_module("test.todo", |_action_json| {
            use crate::substrate::{ActionPlan, ActionStatus};
            Ok(ActionPlan {
                initial_step: serde_json::Value::String("Pending".into()),
                initial_status: ActionStatus::Pending,
                deadline_ms: None,
            })
        });
        app_mut.register_action_executor("test.todo", |_action_json, _send| Ok(()));

        // Now `dispatch_action` should succeed end-to-end.
        let out = dispatch_action_json(
            Some(&*app_mut),
            "test.todo",
            r#"{"create":{"title":"buy milk"}}"#,
        );
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(
            parsed.get("correlation_id").is_some(),
            "expected correlation_id, got: {out}"
        );
        nmp_app_free(app);
    }

    /// A host module validator that returns `Err` rejects the action at the
    /// `start()` phase — `dispatch_action` returns `{"error":…}` carrying the
    /// host's message, and the executor is never reached.
    #[test]
    fn host_registered_module_can_reject_action() {
        use crate::substrate::ActionRejection;
        let app = nmp_app_new();
        // SAFETY: see `host_registered_module_and_executor_enables_dispatch_action`.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action_module("test.todo", |_action_json| {
            Err(ActionRejection::Invalid("host rejected: title required".into()))
        });
        app_mut.register_action_executor("test.todo", |_action_json, _send| Ok(()));

        let out = dispatch_action_json(Some(&*app_mut), "test.todo", "{}");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let err = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected error object, got: {out}"));
        assert!(
            err.contains("host rejected: title required"),
            "rejection message should reach the host, got: {err}"
        );
        nmp_app_free(app);
    }

    /// An executor-only namespace (no module registered) is still rejected by
    /// `dispatch_action`'s `start()` phase — proving the module half is
    /// genuinely required and that PR #60's executor seam alone is not enough
    /// for `dispatch_action`.
    #[test]
    fn executor_only_namespace_is_rejected_by_dispatch_action() {
        let app = nmp_app_new();
        // SAFETY: see `host_registered_module_and_executor_enables_dispatch_action`.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action_executor("test.todo", |_action_json, _send| Ok(()));

        let out = dispatch_action_json(Some(&*app_mut), "test.todo", "{}");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let err = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected error object, got: {out}"));
        assert!(
            err.contains("unknown action namespace"),
            "executor-only namespace must fail start() validation, got: {err}"
        );
        nmp_app_free(app);
    }
}
