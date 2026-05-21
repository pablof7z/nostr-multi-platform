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
//!   the (PR-F-deleted) bespoke `nmp_app_publish_signed_event*` FFI symbols
//!   used to use, plus the workspace-internal
//!   [`crate::NmpApp::publish_signed_explicit`] Marmot seam. The actor
//!   re-verifies the Schnorr signature + id hash (D4 — only the actor loop
//!   signs/publishes; a forged event is rejected, never published) and
//!   routes it through the NIP-65 outbox resolver.
//! * [`PublishAction::Cancel`] is engine-internal — `PublishModule::start`
//!   rejects it, so it is NOT dispatchable through `dispatch_action`. The
//!   publish lifecycle's control plane (cancel / retry) stays on the dedicated
//!   FFI symbols `nmp_app_cancel_publish` / `nmp_app_retry_publish`.
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
use crate::substrate::{ActionContext, ActionRejection, ActionResult};

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
        c_string_argument(namespace).as_deref().unwrap_or(""),
        c_string_argument(action_json).as_deref().unwrap_or(""),
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
/// SCOPE: this C symbol exposes *executor* registration only. The full
/// [`nmp_app_dispatch_action`] path also requires a registered *module*
/// (`ActionRegistry::start` validates the JSON shape against it), so a
/// namespace wired through THIS C symbol alone is reachable by the registry's
/// internal `execute` path but not by `nmp_app_dispatch_action`. The
/// module-side seam is [`nmp_app_register_action_module`] (and its Rust
/// counterpart [`NmpApp::register_action_module`]); a host registers BOTH
/// halves to make a namespace fully reachable via `nmp_app_dispatch_action`.
/// A Rust host such as `nmp-app-chirp` uses the Rust methods directly.
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
    // D6 guard: `nmp.*` namespaces are kernel-owned built-ins. A host
    // overwriting them via FFI would bypass the validated built-in logic
    // (e.g. silently replacing PublishModule's signed-event gate). Rust-level
    // callers are trusted and may call `ActionRegistry::register_executor`
    // directly; the C-ABI path is where the guard lives.
    if ns.starts_with("nmp.") {
        return;
    }
    app.register_action_executor(ns, move |action_json, _correlation_id, _send| {
        // The host executor speaks JSON only. The `_send` actor-command
        // bridge and `_correlation_id` are intentionally unused in v1: a host
        // executor that needs to enqueue an `ActorCommand` (or thread the
        // correlation id onto one) uses a separate mechanism; the seam here
        // proves post-construction registration works.
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
/// `NULL` to **accept** the action (the registry mints a correlation id), or
/// a NUL-terminated C string describing the **rejection** reason. The
/// returned error string is read immediately and copied into an owned Rust
/// `String`; the host owns its lifetime and may free or reuse it after the
/// callback returns.
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
/// accept the action or a NUL-terminated error string to reject. Passing a
/// `NULL` `validator` registers an **accept-all** module: every action under
/// `namespace` is accepted — useful when shape validation lives entirely in
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
    // D6 guard: `nmp.*` namespaces are kernel-owned built-ins. A host
    // overwriting them via FFI would bypass the validated built-in logic
    // (e.g. silently replacing PublishModule's signed-event gate). Rust-level
    // callers are trusted and may call `ActionRegistry::register_with_validator`
    // directly; the C-ABI path is where the guard lives.
    if ns.starts_with("nmp.") {
        return;
    }
    let Some(validate) = validator else {
        // No validator → accept-all: every action is accepted. Shape
        // validation is then the host executor's job.
        app.register_action_module(ns, |_action_json| Ok(()));
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
            // NULL return = accept.
            Ok(())
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
/// `Arc<Mutex<…>>` slot), so — unlike `nmp_app_register_action_executor` —
/// it may be invoked before *or after* `nmp_app_start`. A second
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
        let _: Option<()> = crate::ffi_guard::guard_ffi_callback(
            "action result observer",
            || unsafe { observer(cstr.as_ptr()) },
        );
    });
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
        Ok(correlation_id) => {
            // `start()` is a pure validator and the correlation id is the
            // handle the caller acts on.
            //
            // Execution: `start()` only validated the action. Now drive it
            // through the actor. `execute_action` is namespace-aware; an
            // execution failure is surfaced as `{"error":...}` (D6) and the
            // already-minted correlation id is discarded — a rejected
            // dispatch must not look like an accepted one.
            //
            // The minted `correlation_id` is passed into `execute_action` so
            // an executor whose `ActorCommand` settles asynchronously (the
            // `nmp.publish` `PublishNote` path — the actor signs the event)
            // can thread it onto the command. The publish engine then reports
            // this id in `action_results`, matching the host's spinner
            // key. For pre-signed `Publish` actions the id is redundant
            // (preferred_action_id already bound it to the event id).
            match execute_action(app, namespace, action_json, &correlation_id) {
                Ok(()) => {
                    // Push the "action accepted and enqueued" signal to the
                    // host's result observer, if one is registered. Built-in
                    // executors are fire-and-forget, so `result_json` is
                    // `null`; a host executor that needs to return a value
                    // writes it to a snapshot projection (the pull model).
                    // A no-op when no observer is registered.
                    app.action_registry.deliver_result(ActionResult {
                        correlation_id: correlation_id.clone(),
                        result_json: serde_json::Value::Null,
                    });
                    format!(
                        r#"{{"correlation_id":{}}}"#,
                        json_string(&correlation_id)
                    )
                }
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
///
/// `correlation_id` is the registry-minted action id the caller will return
/// to the host. It is forwarded to the executor so an `ActorCommand` whose
/// terminal verdict must carry this id (the `PublishNote` path) can be built
/// with it.
fn execute_action(
    app: &NmpApp,
    namespace: &str,
    action_json: &str,
    correlation_id: &str,
) -> Result<(), String> {
    app.action_registry
        .execute(namespace, action_json, correlation_id, &|cmd| {
            app.send_cmd(cmd)
        })
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
    /// returns a `correlation_id` string. `PublishAction::PublishNote` is used
    /// because it only needs non-empty content — no signed-event fixture —
    /// and still exercises the full registry → adapter → module path.
    #[test]
    fn dispatch_publish_note_action_returns_correlation_id() {
        with_app(|app| {
            let out = dispatch_action_json(
                Some(app),
                "nmp.publish",
                r#"{"PublishNote":{"content":"smoke-test","reply_to_id":null,"target":"Auto"}}"#,
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

    /// A `Publish` action through `dispatch_action` returns a correlation id
    /// equal to the event's `id`, not a freshly minted registry id.
    ///
    /// This is the round-trip contract: a host that keys a spinner on the
    /// returned `correlation_id` must see the same value in the snapshot's
    /// `action_results` entry `correlation_id` (which equals the publish
    /// engine's `PublishHandle` == event `id`).
    #[test]
    fn dispatch_publish_action_returns_event_id_as_correlation_id() {
        with_app(|app| {
            let event = fixture_signed_event();
            let expected_id = event.id.clone();
            let action = PublishAction::Publish {
                handle: "h1".to_string(),
                event,
                target: PublishTarget::Auto,
            };
            let action_json = serde_json::to_string(&action).unwrap();
            let out = dispatch_action_json(Some(app), "nmp.publish", &action_json);
            let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
            let id = parsed
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("expected correlation_id, got: {out}"));
            assert_eq!(
                id, expected_id,
                "Publish action must return the event.id as correlation_id so \
                 dispatch_action return and action_results share the same identifier"
            );
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
                execute_action(app, "nmp.publish", &action_json, "corr-id").is_ok(),
                "publish execution should not error"
            );
        });
    }

    #[test]
    fn execute_action_publish_note_is_ok_without_actor() {
        with_app(|app| {
            let json =
                r#"{"PublishNote":{"content":"h3","reply_to_id":null,"target":"Auto"}}"#;
            assert!(execute_action(app, "nmp.publish", json, "corr-id").is_ok());
        });
    }

    /// An unrecognized namespace has no executor — `execute_action` returns
    /// `Err` (D6), so a host is never handed a correlation id for an action
    /// that was silently dropped.
    #[test]
    fn execute_action_unknown_namespace_returns_err() {
        with_app(|app| {
            let err = execute_action(app, "nmp.future", "{}", "corr-id")
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
        app_mut.register_action_executor("test.greeting", move |action_json, _correlation_id, _send| {
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
        app_mut.register_action_executor("test.failing", |_action_json, _correlation_id, _send| {
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
        app_mut.register_action_executor("test.greeting", |_json, _correlation_id, _send| Ok(()));

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
        app_mut.register_action_module("test.todo", |_action_json| Ok(()));
        app_mut.register_action_executor("test.todo", |_action_json, _correlation_id, _send| Ok(()));

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
        app_mut.register_action_executor("test.todo", |_action_json, _correlation_id, _send| Ok(()));

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

    use std::sync::Mutex;

    /// THE SEAM PROOF: a host registers an action-result observer, dispatches
    /// an action through `dispatch_action`, and the observer fires with the
    /// SAME `correlation_id` the dispatch call returned. This proves the push
    /// channel is wired end-to-end through the dispatcher — not just the
    /// registry slot in isolation.
    #[test]
    fn dispatch_action_delivers_result_to_observer_with_correlation_id() {
        let seen: Arc<Mutex<Vec<crate::substrate::ActionResult>>> =
            Arc::new(Mutex::new(Vec::new()));
        let seen_in_observer = Arc::clone(&seen);

        with_app(|app| {
            app.register_action_result_observer(move |result| {
                seen_in_observer.lock().unwrap().push(result);
            });

            let out = dispatch_action_json(
                Some(app),
                "nmp.publish",
                r#"{"PublishNote":{"content":"observer-test","reply_to_id":null,"target":"Auto"}}"#,
            );
            let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
            let returned_id = parsed
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .expect("dispatch should return a correlation_id")
                .to_string();

            let captured = seen.lock().unwrap();
            assert_eq!(
                captured.len(),
                1,
                "the result observer should fire exactly once per accepted action"
            );
            assert_eq!(
                captured[0].correlation_id, returned_id,
                "observer correlation_id must match the dispatch return value"
            );
            assert!(
                captured[0].result_json.is_null(),
                "a fire-and-forget built-in executor delivers a null result_json"
            );
        });
    }

    /// A rejected action (unknown namespace) never reaches `execute`, so the
    /// result observer must NOT fire — delivery is gated on `Ok` execution.
    #[test]
    fn dispatch_action_does_not_deliver_result_on_rejection() {
        let fired = Arc::new(AtomicBool::new(false));
        let fired_in_observer = Arc::clone(&fired);

        with_app(|app| {
            app.register_action_result_observer(move |_| {
                fired_in_observer.store(true, Ordering::SeqCst);
            });
            let out = dispatch_action_json(Some(app), "nmp.unknown", "{}");
            assert!(
                out.contains("error"),
                "an unknown namespace must be rejected, got: {out}"
            );
            assert!(
                !fired.load(Ordering::SeqCst),
                "the observer must not fire for a rejected action"
            );
        });
    }

    /// The C-ABI registration entry point: registering an observer through
    /// `nmp_app_register_action_result_observer` and dispatching an action
    /// invokes the C callback with the `{"correlation_id":…}` JSON shape.
    #[test]
    fn c_abi_register_action_result_observer_receives_json() {
        // A `static` slot the C callback writes into — an `extern "C" fn`
        // cannot capture, so the observed JSON is parked here.
        static OBSERVED: Mutex<Option<String>> = Mutex::new(None);

        extern "C" fn observer(json: *const c_char) {
            // SAFETY: per the callback contract `json` is a valid
            // NUL-terminated C string live for the duration of this call.
            let s = unsafe { CStr::from_ptr(json) }
                .to_string_lossy()
                .into_owned();
            *OBSERVED.lock().unwrap() = Some(s);
        }

        *OBSERVED.lock().unwrap() = None;
        let app = nmp_app_new();
        nmp_app_register_action_result_observer(app, Some(observer));
        let out = dispatch_action_json(
            // SAFETY: `nmp_app_new` never returns null.
            Some(unsafe { &*app }),
            "nmp.publish",
            r#"{"PublishNote":{"content":"c-abi-test","reply_to_id":null,"target":"Auto"}}"#,
        );
        let returned_id: serde_json::Value = serde_json::from_str(&out).unwrap();
        let returned_id = returned_id
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .expect("dispatch should return a correlation_id");

        let observed = OBSERVED.lock().unwrap().clone();
        let observed = observed.expect("the C observer callback should have fired");
        let parsed: serde_json::Value = serde_json::from_str(&observed)
            .expect("the observer payload should be valid JSON");
        assert_eq!(
            parsed.get("correlation_id").and_then(|v| v.as_str()),
            Some(returned_id),
            "C observer payload must carry the dispatch correlation_id"
        );
        assert!(
            parsed.get("result_json").map(|v| v.is_null()).unwrap_or(false),
            "C observer payload must carry a result_json field (null here)"
        );
        nmp_app_free(app);
    }

    /// A null `app` or null `observer` is a silent no-op (D6).
    #[test]
    fn c_abi_register_action_result_observer_null_args_are_noop() {
        extern "C" fn observer(_json: *const c_char) {}
        // Null app — must not crash.
        nmp_app_register_action_result_observer(std::ptr::null_mut(), Some(observer));
        // Null observer — must not crash.
        let app = nmp_app_new();
        nmp_app_register_action_result_observer(app, None);
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
        app_mut.register_action_executor("test.todo", |_action_json, _correlation_id, _send| Ok(()));

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

    /// D6 guard: `nmp_app_register_action_executor` silently ignores any
    /// namespace that starts with `"nmp."` — a host cannot shadow a
    /// kernel-owned built-in via the C-ABI and bypass its validation gate.
    /// After an attempted override the built-in `nmp.publish` executor still
    /// handles the action correctly.
    #[test]
    fn c_abi_nmp_prefixed_executor_registration_is_silently_rejected() {
        use std::ffi::CString;

        // A custom executor that always returns a failure message.
        extern "C" fn shadow_executor(_json: *const c_char) -> *const c_char {
            c"shadow_executor_ran".as_ptr()
        }

        let app = nmp_app_new();
        // Attempt to replace the built-in executor via the C-ABI guard.
        let ns = CString::new("nmp.publish").unwrap();
        nmp_app_register_action_executor(app, ns.as_ptr(), Some(shadow_executor));

        // The built-in must still handle `nmp.publish` (`PublishNote` needs no
        // signed event and exercises the full execute path).
        let result = execute_action(
            // SAFETY: nmp_app_new never returns null; valid until nmp_app_free.
            unsafe { &*app },
            "nmp.publish",
            r#"{"PublishNote":{"content":"guard-probe","reply_to_id":null,"target":"Auto"}}"#,
            "corr-id",
        );
        assert!(
            result.is_ok(),
            "built-in executor must still run after rejected nmp.* registration, got: {result:?}"
        );
        nmp_app_free(app);
    }

    /// D6 guard: `nmp_app_register_action_module` silently ignores any
    /// namespace starting with `"nmp."` — the kernel-owned module validator
    /// cannot be replaced via the C-ABI.
    #[test]
    fn c_abi_nmp_prefixed_module_registration_is_silently_rejected() {
        use std::ffi::CString;

        // A custom validator that always accepts everything (accept-all null
        // validator path — would bypass PublishModule's signed-event gate).
        let app = nmp_app_new();
        let ns = CString::new("nmp.publish").unwrap();
        nmp_app_register_action_module(app, ns.as_ptr(), None);

        // PublishModule's validation gate is still in force — the C-ABI
        // registration with a `None` validator was silently rejected, so the
        // built-in `PublishModule::start` (not an accept-all replacement) runs.
        // A well-formed `PublishNote` exercises that gate and is accepted:
        let out = dispatch_action_json(
            // SAFETY: nmp_app_new never returns null; valid until nmp_app_free.
            Some(unsafe { &*app }),
            "nmp.publish",
            r#"{"PublishNote":{"content":"guard-probe","reply_to_id":null,"target":"Auto"}}"#,
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("dispatch always returns JSON");
        // `PublishNote` must still be accepted by the built-in module
        // (correlation_id, not an error) — the accept-all replacement did NOT
        // take effect.
        assert!(
            parsed.get("correlation_id").is_some(),
            "built-in module must still validate after rejected nmp.* registration, got: {out}"
        );
        nmp_app_free(app);
    }
}
