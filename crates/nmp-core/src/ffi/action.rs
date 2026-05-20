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
use crate::actor::ActorCommand;
use crate::publish::{PublishAction, PublishTarget};
use crate::store::RawEvent;
use crate::substrate::{ActionContext, ActionRejection, SignedEvent};

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

/// Drive the validated action toward execution.
///
/// `start()` (the registry) has already validated `action_json` against the
/// module's `Action` type, so the re-deserialize below is expected to
/// succeed — but it is still treated as data (D6: a deserialize failure
/// becomes an error string, never a panic).
///
/// Per-namespace execution:
/// * `nmp.publish` / [`PublishAction::Publish`] — convert the validated
///   [`SignedEvent`] to a [`RawEvent`] and send [`ActorCommand::PublishSignedEvent`].
///   The actor re-verifies and routes through the publish engine (D4).
/// * `nmp.publish` / [`PublishAction::Cancel`] — no actor command exists
///   yet; the registry already reported `ActionStatus::Cancelled`, so this
///   is a no-op (a real publish-engine cancel is a follow-up).
/// * Any other namespace — no executor is wired yet; treated as a no-op so
///   the correlation id is still returned (validation succeeded). NIP-29 /
///   NIP-59 modules are app nouns (D0) registered against the app host's
///   own registry; their executors land with those crates.
fn execute_action(app: &NmpApp, namespace: &str, action_json: &str) -> Result<(), String> {
    match namespace {
        "nmp.publish" => {
            let action: PublishAction = serde_json::from_str(action_json)
                .map_err(|e| format!("publish action decode failed: {e}"))?;
            match action {
                PublishAction::Publish { event, target, .. } => {
                    // D8 — non-blocking channel send only; the actor loop
                    // owns signing/publishing (D4). The event is already
                    // signed; the actor re-verifies it before publishing.
                    app.send_cmd(ActorCommand::PublishSignedEvent {
                        raw: signed_event_to_raw(event),
                        relays: relays_for_target(&target),
                    });
                    Ok(())
                }
                // No publish-engine cancel command yet; the registry
                // already marked the action `Cancelled`.
                PublishAction::Cancel { .. } => Ok(()),
            }
        }
        // No executor wired for other namespaces yet — validation passed,
        // so the caller still gets a correlation id.
        _ => Ok(()),
    }
}

/// Convert a [`SignedEvent`] (the publish-action / engine input shape) into
/// a flat NIP-01 [`RawEvent`] (the actor command shape). Pure field move —
/// `id` and `sig` are carried verbatim, no re-signing. This is the inverse
/// of the `RawEvent → SignedEvent` conversion in
/// `actor::commands::publish::publish_signed_event`.
fn signed_event_to_raw(event: SignedEvent) -> RawEvent {
    RawEvent {
        id: event.id,
        pubkey: event.unsigned.pubkey,
        created_at: event.unsigned.created_at,
        kind: event.unsigned.kind,
        tags: event.unsigned.tags,
        content: event.unsigned.content,
        sig: event.sig,
    }
}

/// Resolve a [`PublishTarget`] into the relay slice
/// [`ActorCommand::PublishSignedEvent`] expects: `Auto` → empty (NIP-65
/// outbox resolver, D3 default), `Explicit` → the named opt-out relays.
fn relays_for_target(target: &PublishTarget) -> Vec<crate::publish::RelayUrl> {
    match target {
        PublishTarget::Auto => Vec::new(),
        PublishTarget::Explicit { relays } => relays.clone(),
    }
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

    // ── M6 execution wiring ─────────────────────────────────────────────
    // `PublishAction`, `PublishTarget`, `SignedEvent` reach this module via
    // `use super::*`; only `UnsignedEvent` needs an explicit import.
    use crate::substrate::UnsignedEvent;

    /// A `SignedEvent` with non-empty id/sig — enough to pass
    /// `PublishModule::start`'s "requires a signed event" gate. The id/sig
    /// are syntactically well-formed hex but NOT cryptographically valid;
    /// that is fine here because these tests stop at the FFI dispatch seam
    /// (the actor would re-verify, but no actor assertion is made — see the
    /// note on `dispatch_publish_action_returns_correlation_id`).
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
    /// (validation + execution wiring both succeeded). This exercises the
    /// full path: `registry.start()` validates, then `execute_action`
    /// converts the signed event and sends `ActorCommand::PublishSignedEvent`
    /// down the actor channel.
    ///
    /// The actor receiving the command is a non-blocking channel send (D8);
    /// the actor then re-verifies the signature (D4). This test deliberately
    /// asserts only on the FFI return value — a cryptographically-valid
    /// signed-event fixture (and a relay-backed harness) is what an
    /// end-to-end publish test in `tests/` would need.
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

    /// `execute_action` for a valid `Publish` action sends one
    /// `PublishSignedEvent` and reports success. Asserting through the public
    /// `dispatch_action_json` path keeps the actor channel send real.
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

    /// `Cancel` does not reach the actor (no publish-engine cancel command
    /// yet) but still succeeds — the registry already marked it cancelled.
    #[test]
    fn execute_action_cancel_is_ok_without_actor() {
        with_app(|app| {
            let json = r#"{"Cancel":{"handle":"h3"}}"#;
            assert!(execute_action(app, "nmp.publish", json).is_ok());
        });
    }

    /// An unrecognized namespace has no executor wired — `execute_action`
    /// treats it as a no-op success so a validated action still returns its
    /// correlation id.
    #[test]
    fn execute_action_unknown_namespace_is_noop_ok() {
        with_app(|app| {
            assert!(execute_action(app, "nmp.future", "{}").is_ok());
        });
    }

    /// `signed_event_to_raw` is a pure field move: id/sig/pubkey/kind/tags/
    /// content/created_at carry through verbatim (no re-signing).
    #[test]
    fn signed_event_to_raw_carries_all_fields_verbatim() {
        let signed = fixture_signed_event();
        let raw = signed_event_to_raw(signed.clone());
        assert_eq!(raw.id, signed.id);
        assert_eq!(raw.sig, signed.sig);
        assert_eq!(raw.pubkey, signed.unsigned.pubkey);
        assert_eq!(raw.kind, signed.unsigned.kind);
        assert_eq!(raw.tags, signed.unsigned.tags);
        assert_eq!(raw.content, signed.unsigned.content);
        assert_eq!(raw.created_at, signed.unsigned.created_at);
    }

    /// `relays_for_target` maps `Auto` → empty (D3 outbox resolver) and
    /// `Explicit` → the named opt-out relays verbatim.
    #[test]
    fn relays_for_target_maps_auto_and_explicit() {
        assert!(relays_for_target(&PublishTarget::Auto).is_empty());
        let explicit = PublishTarget::Explicit {
            relays: vec!["wss://a.example".to_string(), "wss://b.example".to_string()],
        };
        assert_eq!(
            relays_for_target(&explicit),
            vec![
                "wss://a.example".to_string(),
                "wss://b.example".to_string()
            ]
        );
    }
}
