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

use super::{app_ref, NmpApp};
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
}
