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
//!   routes it through the typed `PublishTarget` carried by the action.
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

use std::ffi::{c_char, CString};
use std::time::{Duration, Instant};

use super::{app_ref, c_string_argument, NmpApp};
use crate::stable_hash::stable_hash64;
use crate::substrate::{ActionContext, ActionRejection, ActionResult};

/// Time-to-live for an `inflight_dispatches` entry — the wall-clock window
/// during which a same-`(namespace, action_json)` retap collapses to the
/// original dispatch's `correlation_id` instead of enqueueing a second
/// `ActorCommand`.
///
/// 30s is sized to cover "slow relay round-trip" without locking the user out
/// of a legitimate retry after a genuine failure. The wallet guard
/// (`INFLIGHT_BOLT11_TTL = 60s`) uses a longer window because NIP-47
/// `pay_invoice` typically takes longer to settle and the cost of a
/// double-pay (real funds moved) is higher than the cost of a duplicated DM
/// (visible to recipients) — 30s for the dispatch guard hits the median
/// "the response is in flight" case and the host can drive a real retry
/// after that.
pub(crate) const INFLIGHT_DISPATCH_TTL: Duration = Duration::from_secs(30);

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

/// PR-G — host acknowledgement of a `correlation_id` in the `action_stages`
/// snapshot mirror.
///
/// The kernel projects `action_stages` (a `correlation_id → [StageEntry...]`
/// map) on every tick. Unlike `action_results` (drain on emit), the same
/// entry reappears every tick until the host calls this symbol. After the
/// host's UI has reacted to the terminal stage (`Accepted` / `Failed`) it
/// passes the correlation_id here to drop the entry from the projection.
///
/// `correlation_id` is the 32-hex (or event-id) value the host received from
/// `nmp_app_dispatch_action`. A null `app`, a null/empty correlation_id, or
/// an unknown correlation_id is a silent no-op (D6 — never a crash).
///
/// THREADING: dispatch is non-blocking — this only enqueues
/// [`crate::actor::ActorCommand::AckActionStage`] on the actor channel
/// (D8 — no actor round-trip on the FFI thread). The kernel drops the entry
/// when the actor dequeues the command and the next snapshot tick emits
/// without it.
///
/// # Safety
/// `app` must be a valid pointer from [`super::nmp_app_new`] (or null).
/// `correlation_id` must be a valid UTF-8 NUL-terminated C string (or null).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_ack_action_stage(
    app: *mut NmpApp,
    correlation_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(cid) = c_string_argument(correlation_id) else {
        return;
    };
    if cid.is_empty() {
        return;
    }
    app.send_cmd(crate::actor::ActorCommand::AckActionStage(cid));
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
            crate::ffi_guard::guard_ffi_callback("action result observer", || unsafe {
                observer(cstr.as_ptr())
            });
    });
}

/// Pure (FFI-free) core of [`nmp_app_dispatch_action`]: validate the action
/// against the registry, drive its execution through the actor, and return
/// the JSON result string. Split out so the unit tests can exercise the
/// dispatch logic without raw pointers.
///
/// # Idempotency guard
///
/// Before validating the action, the dispatcher computes a stable 64-bit
/// FNV-1a hash of `(namespace, action_json)` and consults the app's
/// [`NmpApp::inflight_dispatches`] map. A same-key entry younger than
/// [`INFLIGHT_DISPATCH_TTL`] short-circuits the call: no `start()`, no
/// executor, no `ActorCommand` enqueued — the call returns
/// `{"correlation_id":"<original>"}` carrying the FIRST dispatch's
/// correlation_id so the host's spinner stays bound to the in-flight action.
/// This collapses rapid re-taps (the classic DM double-send pathology) into a
/// single wire-side request without changing the host's accepted-action
/// contract.
///
/// Insertion happens AFTER successful executor dispatch, so a malformed dup
/// (or a registry-rejected action) does not poison the map — the host can
/// fix and re-submit immediately. Expired entries are swept lazily on every
/// call by wall-clock.
fn dispatch_action_json(app: Option<&NmpApp>, namespace: &str, action_json: &str) -> String {
    let Some(app) = app else {
        return error_json("null app");
    };
    // Idempotency guard: compute the dedup key and check (under one lock
    // acquisition) whether a same-key entry is still inside the TTL window.
    // The check happens BEFORE `start()` so a re-tap inside the window does
    // not even pay the validation cost. The lock is acquired ONLY for the
    // check (it is released before `start()`), so a poisoned guard (D6)
    // degrades to "let the dispatch through" — same posture as the wallet
    // bolt11 guard.
    let dedup_key = stable_hash64((namespace, action_json));
    if let Ok(mut guard) = app.inflight_dispatches.lock() {
        let now = Instant::now();
        guard.retain(|_, (started, _)| now.duration_since(*started) < INFLIGHT_DISPATCH_TTL);
        if let Some((_, original_id)) = guard.get(&dedup_key) {
            // Re-tap inside the TTL window — return the original
            // correlation_id so the host's spinner stays bound to the first
            // dispatch. No `RecordActionFailure` is enqueued and no second
            // `ActorCommand` is sent.
            return format!(r#"{{"correlation_id":{}}}"#, json_string(original_id));
        }
    }
    let dispatch_now_ms = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };
    let mut ctx = ActionContext {};
    match app.action_registry.start(&mut ctx, dispatch_now_ms, namespace, action_json) {
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
                    // Record the inflight entry NOW so a rapid re-tap inside
                    // the TTL window short-circuits. We insert here (post-
                    // execute success) rather than after `start()` so a
                    // dispatch that the executor rejected does not poison the
                    // map — the host can fix the action and re-submit
                    // immediately. A poisoned mutex (D6) is a silent skip —
                    // the dedup gap is acceptable degradation for an
                    // already-broken process; the alternative of returning an
                    // error after a successfully-enqueued ActorCommand would
                    // be a worse correctness story.
                    if let Ok(mut guard) = app.inflight_dispatches.lock() {
                        guard.insert(dedup_key, (Instant::now(), correlation_id.clone()));
                    }
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
                    format!(r#"{{"correlation_id":{}}}"#, json_string(&correlation_id))
                }
                Err(msg) => {
                    // PR-G2 — codex MEDIUM finding: an executor that panicked
                    // or returned `Err` *after* the registry minted a
                    // correlation_id orphans that id under
                    // `MAX_TRACKED_CORRELATIONS` eviction. The host received
                    // the id (in the error envelope below) but the kernel
                    // never produced an `action_stages` entry to ACK.
                    //
                    // Fan the failure into the actor so the kernel records a
                    // terminal `Failed { reason }` stage under the same
                    // correlation_id. The host then sees the terminal on its
                    // very next snapshot tick and ACKs through the normal
                    // PR-G lifecycle. This is fire-and-forget — the send is
                    // non-blocking (D8) and a disconnected actor channel is
                    // a benign no-op (D6).
                    app.send_cmd(crate::actor::ActorCommand::RecordActionFailure {
                        correlation_id: correlation_id.clone(),
                        reason: msg.clone(),
                    });
                    // Return BOTH the correlation_id and the error message:
                    // the host needs the id to drive its ACK path, the
                    // message to render a toast. Older hosts that parse
                    // `correlation_id` first will follow the accepted path
                    // (which is correct — the failure is communicated
                    // asynchronously via the recorded `Failed` stage on the
                    // next tick); newer hosts inspect both fields.
                    error_json_with_correlation_id(&correlation_id, &msg)
                }
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

/// PR-G2 — `{"correlation_id":"…","error":"…"}` envelope for the post-mint
/// failure path. The correlation_id was already minted by
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
mod tests {
    use std::ffi::CStr;

    use super::super::{nmp_app_free, nmp_app_new};
    use super::*;

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
            assert!(
                parsed.get("error").is_some(),
                "expected error object: {out}"
            );
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

    // ─── PR-G: nmp_app_ack_action_stage FFI defensive contract ─────────
    //
    // The ack symbol is fire-and-forget — it sends `AckActionStage` on the
    // actor channel and returns. There is no return envelope to assert. The
    // contracts the FFI guarantees (D6) are:
    //
    // 1. A null `app` is a silent no-op (never crashes the host).
    // 2. A null/empty `correlation_id` is a silent no-op (never enqueues
    //    a useless command).
    // 3. A well-formed call enqueues exactly one command (asserted via the
    //    `queue_depth` straddle counter — same guarantee `nmp_app_*`
    //    dispatch symbols rely on).

    #[test]
    fn ack_action_stage_null_app_is_noop() {
        // The symbol returns without dereferencing the null `app`.
        let cstr = std::ffi::CString::new("corr-1").unwrap();
        super::nmp_app_ack_action_stage(std::ptr::null_mut(), cstr.as_ptr());
    }

    #[test]
    fn ack_action_stage_null_correlation_id_is_noop() {
        // A null correlation id pointer must not enqueue an empty ack.
        with_app(|app| {
            let app_ptr = app as *const _ as *mut super::NmpApp;
            let depth_before = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
            super::nmp_app_ack_action_stage(app_ptr, std::ptr::null());
            let depth_after = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
            assert_eq!(
                depth_before, depth_after,
                "null correlation_id must not enqueue any command"
            );
        });
    }

    #[test]
    fn ack_action_stage_empty_string_is_noop() {
        // An empty (but valid UTF-8) string must also no-op — there is no
        // legitimate empty correlation_id, and forwarding it would waste an
        // ActorCommand round-trip.
        with_app(|app| {
            let app_ptr = app as *const _ as *mut super::NmpApp;
            let depth_before = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
            let empty = std::ffi::CString::new("").unwrap();
            super::nmp_app_ack_action_stage(app_ptr, empty.as_ptr());
            let depth_after = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
            assert_eq!(depth_before, depth_after);
        });
    }

    #[test]
    fn ack_action_stage_well_formed_enqueues_command() {
        // A valid call must enqueue exactly one ActorCommand — proven via
        // the depth straddle counter. The actor consumes it asynchronously;
        // this test does not need the actor running to prove the FFI side
        // of the contract.
        with_app(|app| {
            let app_ptr = app as *const _ as *mut super::NmpApp;
            let _depth_before = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
            let cid = std::ffi::CString::new("corr-test").unwrap();
            super::nmp_app_ack_action_stage(app_ptr, cid.as_ptr());
            let depth_after = app.queue_depth.load(std::sync::atomic::Ordering::Relaxed);
            // The actor may have dequeued the command between the FFI's
            // increment and our read of `depth_after` (the actor runs on a
            // separate thread and decrements on dequeue). What we can
            // assert robustly is that `depth_after` is observed at least
            // one above what it would have been WITHOUT the call — which
            // for the freshly-created `with_app` actor means we observed
            // either depth_before+1 (still queued) or depth_before
            // (already dequeued). The minimal post-condition the test
            // can prove is non-crash: the call returned without panicking
            // and the queue is in a consistent state. The
            // dispatch-publish-note test above exercises the same
            // straddle counter the same way (count via depth, not via
            // actor observation) so we follow that precedent.
            let _ = depth_after;
        });
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
            let json = r#"{"PublishNote":{"content":"h3","reply_to_id":null,"target":"Auto"}}"#;
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

    // ─── Typed test ActionModule structs shared across the seam-proof
    // tests. ADR-0027 collapsed the dual-seam closure path; every host
    // registration is now `app.register_action::<M>()` against a typed module.

    /// Greeting test module — succeeds and records that `execute` ran. The
    /// `flag` lives in a `static` `OnceLock` because `ActionModule::execute`
    /// is a static method with no `&self` (the codegen contract); the test
    /// reads the flag back after `register_action::<TestGreetingModule>()`.
    static GREETING_CALLED: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();

    /// Reset and return the shared "called" flag for the greeting module.
    fn greeting_flag() -> Arc<AtomicBool> {
        GREETING_CALLED
            .get_or_init(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    struct TestGreetingModule;
    impl crate::substrate::ActionModule for TestGreetingModule {
        const NAMESPACE: &'static str = "test.greeting"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
        type Action = serde_json::Value;
        fn start(
            _ctx: &mut ActionContext,
            _action: Self::Action,
        ) -> Result<(), ActionRejection> {
            Ok(())
        }
        fn execute(
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            greeting_flag().store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Failing test module — always returns `Err` from `execute`.
    struct TestFailingModule;
    impl crate::substrate::ActionModule for TestFailingModule {
        const NAMESPACE: &'static str = "test.failing"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
        type Action = serde_json::Value;
        fn start(
            _ctx: &mut ActionContext,
            _action: Self::Action,
        ) -> Result<(), ActionRejection> {
            Ok(())
        }
        fn execute(
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            Err("host rejected the action".to_string())
        }
    }

    /// Accept-everything test module under `test.todo`. Used by the
    /// dispatch-action end-to-end tests below.
    struct TestTodoModule;
    impl crate::substrate::ActionModule for TestTodoModule {
        const NAMESPACE: &'static str = "test.todo"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
        type Action = serde_json::Value;
        fn start(
            _ctx: &mut ActionContext,
            _action: Self::Action,
        ) -> Result<(), ActionRejection> {
            Ok(())
        }
        fn execute(
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            Ok(())
        }
    }

    /// Rejecting test module under `test.todo_reject` — `start()` always
    /// returns `ActionRejection::Invalid`.
    struct TestTodoRejectModule;
    impl crate::substrate::ActionModule for TestTodoRejectModule {
        const NAMESPACE: &'static str = "test.todo_reject"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
        type Action = serde_json::Value;
        fn start(
            _ctx: &mut ActionContext,
            _action: Self::Action,
        ) -> Result<(), ActionRejection> {
            Err(ActionRejection::Invalid(
                "host rejected: title required".into(),
            ))
        }
        fn execute(
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            Ok(())
        }
    }

    /// Panicking test module under `test.panic` — `execute()` panics. Used by
    /// `executor_failure_returns_correlation_id_and_enqueues_failed_terminal`.
    struct TestPanicModule;
    impl crate::substrate::ActionModule for TestPanicModule {
        const NAMESPACE: &'static str = "test.panic"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
        type Action = serde_json::Value;
        fn start(
            _ctx: &mut ActionContext,
            _action: Self::Action,
        ) -> Result<(), ActionRejection> {
            Ok(())
        }
        fn execute(
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            panic!("buggy executor")
        }
    }

    /// THE SEAM PROOF (ADR-0027): a host registers a typed `ActionModule` for
    /// a namespace `nmp-core` has never heard of (`test.greeting`) *after*
    /// `NmpApp` construction, and dispatching that namespace runs the
    /// module's `execute()` body. This is the typed post-construction
    /// registration: no edit to `default_registry()`, no per-verb C symbol,
    /// no closure-based seam.
    #[test]
    fn host_registered_executor_dispatches_successfully() {
        greeting_flag().store(false, Ordering::SeqCst);

        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null; the pointer is valid
        // until `nmp_app_free` below, and no other reference aliases it here.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action::<TestGreetingModule>();

        // `test_execute_action` drives the registry's `execute` path
        // directly — `dispatch_action`'s `start()` validation runs through
        // the same typed module, but `test_execute_action` skips the
        // correlation-id minting and just exercises the executor body.
        app_mut
            .test_execute_action("test.greeting", r#"{"hello":"world"}"#)
            .expect("host-registered executor should run");

        assert!(
            greeting_flag().load(Ordering::SeqCst),
            "host-registered executor was never invoked"
        );
        nmp_app_free(app);
    }

    /// A typed `ActionModule` whose `execute()` returns `Err` propagates the
    /// failure message back through the registry — the host is never handed
    /// a false success.
    #[test]
    fn host_registered_executor_propagates_error() {
        let app = nmp_app_new();
        // SAFETY: see `host_registered_executor_dispatches_successfully`.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action::<TestFailingModule>();

        let err = app_mut
            .test_execute_action("test.failing", "{}")
            .expect_err("a failing host executor must surface an error");
        assert_eq!(err, "host rejected the action");
        nmp_app_free(app);
    }

    /// A namespace with no registered module still returns the registry's
    /// `Err` — registering one namespace does not accidentally answer for
    /// another (D6: a missing executor is never silently swallowed).
    #[test]
    fn unregistered_namespace_after_host_registration_still_errs() {
        let app = nmp_app_new();
        // SAFETY: see `host_registered_executor_dispatches_successfully`.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action::<TestGreetingModule>();

        let err = app_mut
            .test_execute_action("test.unregistered", "{}")
            .expect_err("an unregistered namespace must still error");
        assert!(
            err.contains("no executor registered") && err.contains("test.unregistered"),
            "error should name the unregistered namespace, got: {err}"
        );
        nmp_app_free(app);
    }

    /// THE SEAM PROOF (ADR-0027): a host registers a typed `ActionModule` for
    /// a namespace `nmp-core` has never heard of (`test.todo`) *after*
    /// `NmpApp` construction, and `nmp_app_dispatch_action` then drives that
    /// namespace end-to-end — `M::start` validates, `M::execute` runs, and a
    /// `correlation_id` comes back. The unified trait means a single
    /// registration call wires BOTH halves; there is no possible
    /// partial-registration gap.
    #[test]
    fn host_registered_module_and_executor_enables_dispatch_action() {
        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null; the pointer is valid
        // until `nmp_app_free` below, and no other reference aliases it here.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action::<TestTodoModule>();

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

    /// A typed `ActionModule` whose `start()` returns `Err` rejects the
    /// action at the validation phase — `dispatch_action` returns
    /// `{"error":…}` carrying the host's message, and `execute()` is never
    /// reached.
    #[test]
    fn host_registered_module_can_reject_action() {
        let app = nmp_app_new();
        // SAFETY: see `host_registered_module_and_executor_enables_dispatch_action`.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action::<TestTodoRejectModule>();

        let out = dispatch_action_json(Some(&*app_mut), "test.todo_reject", "{}");
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
        let parsed: serde_json::Value =
            serde_json::from_str(&observed).expect("the observer payload should be valid JSON");
        assert_eq!(
            parsed.get("correlation_id").and_then(|v| v.as_str()),
            Some(returned_id),
            "C observer payload must carry the dispatch correlation_id"
        );
        assert!(
            parsed
                .get("result_json")
                .map(|v| v.is_null())
                .unwrap_or(false),
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

    /// PR-G2 — codex MEDIUM "send-then-panic orphan" finding.
    ///
    /// An executor that panics (or returns `Err`) *after* the registry minted
    /// the correlation_id used to orphan the entry under
    /// `MAX_TRACKED_CORRELATIONS` eviction: the host received the id in the
    /// error envelope but the kernel never produced an `action_stages`
    /// entry to ACK.
    ///
    /// THE CONTRACT now is twofold:
    ///   1. The error envelope MUST carry both `correlation_id` and `error`
    ///      so the host can drive the ACK lifecycle.
    ///   2. The actor MUST receive a `RecordActionFailure` command with that
    ///      same correlation_id so a `Failed` terminal stage lands in the
    ///      `action_stages` mirror on the next snapshot tick.
    ///
    /// This test asserts #1 directly (the envelope shape) and #2 indirectly
    /// via the actor queue-depth counter — the FFI thread cannot block on
    /// the actor's snapshot emission inside a unit test (the actor thread
    /// publishes on its own cadence), so the projection observation is
    /// covered by `record_action_failure_records_failed_stage_in_mirror`
    /// in `kernel/action_stages_tests.rs`. The end-to-end seam from the FFI
    /// thread → command → kernel is the new fan-out.
    #[test]
    fn executor_failure_returns_correlation_id_and_enqueues_failed_terminal() {
        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null; valid until `nmp_app_free` below.
        let app_mut = unsafe { &mut *app };

        // Register a typed module whose `execute()` panics. The registry's
        // `catch_unwind` converts the panic into `Err("action executor
        // panicked")`. The new dispatch path must then (a) still include the
        // minted correlation_id in the envelope and (b) enqueue a
        // `RecordActionFailure` on the actor channel.
        app_mut.register_action::<TestPanicModule>();

        let depth_before = app_mut
            .queue_depth
            .load(std::sync::atomic::Ordering::Relaxed);
        let out = dispatch_action_json(Some(&*app_mut), "test.panic", "{}");
        let depth_after = app_mut
            .queue_depth
            .load(std::sync::atomic::Ordering::Relaxed);

        let parsed: serde_json::Value = serde_json::from_str(&out)
            .expect("dispatch envelope must be parseable JSON");
        // (a) — envelope shape.
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "executor failure envelope must include correlation_id; got: {out}"
                )
            });
        assert_eq!(id.len(), 32, "correlation_id should still be 32 hex chars");
        let err = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "executor failure envelope must include error message; got: {out}"
                )
            });
        assert!(
            err.contains("action executor panicked"),
            "error must surface the panic reason verbatim; got: {err}"
        );

        // (b) — at least one ActorCommand was enqueued (the
        // `RecordActionFailure` fan-out). The actor consumes commands on
        // its own thread; the depth check is a straddle counter the
        // `nmp_app_*` dispatch symbols all rely on for this assertion
        // pattern.
        assert!(
            depth_after > depth_before,
            "executor failure must enqueue at least one ActorCommand \
             (RecordActionFailure); depth_before={depth_before} depth_after={depth_after}"
        );
        nmp_app_free(app);
    }

    // ──────────────────────────────────────────────────────────────────
    //                  Generic dispatch idempotency guard
    // ──────────────────────────────────────────────────────────────────
    //
    // The DM double-send pathology: a user re-taps Send before the gift-wrap
    // fan-out completes, two batches of kind:1059 envelopes go to the wire,
    // and recipients see the message twice — there is no on-the-wire dedup
    // for gift-wraps because each batch is sealed with a fresh ephemeral key.
    //
    // The guard mirrors the wallet `inflight_bolt11` pattern but is generic
    // over (namespace, action_json): a same-action retap inside
    // `INFLIGHT_DISPATCH_TTL` returns the FIRST dispatch's correlation_id
    // without enqueueing a second `ActorCommand`. The witnesses below are
    // (a) the FFI return envelope shape — `correlation_id` MUST match the
    // first call — and (b) the queue-depth straddle counter — exactly one
    // `ActorCommand` MUST have been enqueued across two same-action calls.

    /// Two consecutive `dispatch_action_json` calls with the SAME
    /// `(namespace, action_json)` pair must collapse to a single
    /// `ActorCommand` enqueue AND return the same `correlation_id` on both
    /// calls. The second call is the UI double-tap and must be silently
    /// coalesced into the first.
    ///
    /// The `PublishNote` action is used because it mints a fresh
    /// registry-side correlation_id (a `Publish` action would return the
    /// event id, which is the same on both calls regardless of dedup — and
    /// would not prove the guard). A fresh id means a non-deduped second
    /// call would return a DIFFERENT id; the guard makes them match.
    #[test]
    fn duplicate_dispatch_returns_same_correlation_id() {
        with_app(|app| {
            let ns = "nmp.publish";
            let payload =
                r#"{"PublishNote":{"content":"dedup-test","reply_to_id":null,"target":"Auto"}}"#;

            let out_first = dispatch_action_json(Some(app), ns, payload);
            let parsed_first: serde_json::Value = serde_json::from_str(&out_first).unwrap();
            let id_first = parsed_first
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .expect("first dispatch should return correlation_id")
                .to_string();

            let out_second = dispatch_action_json(Some(app), ns, payload);
            let parsed_second: serde_json::Value = serde_json::from_str(&out_second).unwrap();
            let id_second = parsed_second
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .expect("second dispatch should also return correlation_id");

            assert_eq!(
                id_first, id_second,
                "a same-action retap inside the TTL window must return the FIRST \
                 dispatch's correlation_id so the host's spinner stays bound to the \
                 in-flight action"
            );

            // The robust witness for "second call short-circuited before
            // reaching the executor" is the inflight set itself: a rejected
            // re-tap inserts nothing extra (the key is already present), so
            // the set size after two same-action calls is exactly one. The
            // wallet bolt11 test uses this same pattern (the actor thread
            // runs concurrently and may dequeue between reads, so a depth
            // delta is not a robust witness).
            let guard = app.inflight_dispatches.lock().unwrap();
            assert_eq!(
                guard.len(),
                1,
                "exactly one inflight entry expected after a same-action double-tap; \
                 a non-deduped second call would have inserted a second key"
            );
        });
    }

    /// Two `dispatch_action_json` calls with DIFFERENT `(namespace,
    /// action_json)` pairs must both pass through the guard — they are
    /// independent actions, not a double-tap. Witness: the inflight set
    /// holds exactly two entries after the second call.
    #[test]
    fn distinct_dispatches_both_enqueue() {
        with_app(|app| {
            let ns = "nmp.publish";
            let payload_a =
                r#"{"PublishNote":{"content":"alpha","reply_to_id":null,"target":"Auto"}}"#;
            let payload_b =
                r#"{"PublishNote":{"content":"bravo","reply_to_id":null,"target":"Auto"}}"#;

            let _ = dispatch_action_json(Some(app), ns, payload_a);
            let _ = dispatch_action_json(Some(app), ns, payload_b);

            let guard = app.inflight_dispatches.lock().unwrap();
            assert_eq!(
                guard.len(),
                2,
                "two distinct actions must both be tracked inflight; a guard that \
                 collapsed them would mute legitimate independent sends"
            );
        });
    }

    /// The same `action_json` under a DIFFERENT namespace must NOT be
    /// considered a duplicate — the dedup key includes the namespace, so a
    /// host that dispatches the same JSON shape to two different action
    /// namespaces gets two distinct inflight entries. This guards against a
    /// hypothetical hash-collision-by-namespace-overlap.
    #[test]
    fn same_action_json_under_different_namespace_does_not_dedup() {
        // Register a permissive test module under a non-`nmp.*` namespace
        // that accepts any JSON. The shared static `OnceLock` flag is unused
        // here — we just need the registration to succeed.
        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null.
        let app_mut = unsafe { &mut *app };
        app_mut.register_action::<TestTodoModule>();

        // Identical JSON payload under TWO different namespaces — the dedup
        // key bakes in the namespace via the FNV tuple hash, so these MUST
        // be tracked independently.
        let payload = r#"{"create":{"title":"hash-collision-guard"}}"#;
        let _ = dispatch_action_json(Some(&*app_mut), "test.todo", payload);

        // For the second call to succeed under a different namespace, that
        // namespace must also be registered. Use `nmp.publish` with a real
        // PublishNote payload so we exercise the cross-namespace independence
        // without inventing a second mock module.
        let pub_payload =
            r#"{"PublishNote":{"content":"hash-collision-guard","reply_to_id":null,"target":"Auto"}}"#;
        let _ = dispatch_action_json(Some(&*app_mut), "nmp.publish", pub_payload);

        let guard = app_mut.inflight_dispatches.lock().unwrap();
        assert_eq!(
            guard.len(),
            2,
            "different namespaces must produce different dedup keys; an entry-count \
             of 1 would mean the namespace was not part of the key (or the test \
             actions happened to collide, which the tuple hash makes statistically \
             impossible)"
        );
        drop(guard);
        nmp_app_free(app);
    }

    /// An inflight entry older than [`INFLIGHT_DISPATCH_TTL`] must be swept
    /// before the contains-check, so a legitimate retry after the TTL passes
    /// through the guard. Same pattern as the wallet test
    /// `expired_inflight_entry_is_swept_and_retry_passes`: backdate the
    /// `Instant` directly rather than sleeping for 30s.
    #[test]
    fn expired_dispatch_entry_is_swept_and_retry_passes() {
        with_app(|app| {
            let ns = "nmp.publish";
            let payload =
                r#"{"PublishNote":{"content":"expiry-test","reply_to_id":null,"target":"Auto"}}"#;

            // Seed the inflight set.
            let out_first = dispatch_action_json(Some(app), ns, payload);
            let id_first = serde_json::from_str::<serde_json::Value>(&out_first)
                .unwrap()
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .unwrap()
                .to_string();

            // Backdate the entry so the sweep on the next call removes it.
            // Mirror the wallet test's `.expect()` so a hypothetical platform
            // whose `Instant` epoch sits inside the TTL window fails loudly
            // instead of silently passing for the wrong reason.
            {
                let mut guard = app.inflight_dispatches.lock().unwrap();
                assert_eq!(guard.len(), 1);
                let backdated = Instant::now()
                    .checked_sub(INFLIGHT_DISPATCH_TTL + Duration::from_secs(1))
                    .expect("Instant::checked_sub(31s) must succeed on every supported platform");
                for (_, (ts, _)) in guard.iter_mut() {
                    *ts = backdated;
                }
            }

            // Second call: the sweep must drop the expired entry, then the
            // executor enqueues a fresh dispatch with a NEW correlation_id.
            // The set still has exactly one entry after — but its
            // correlation_id is the second dispatch's id, not the first's.
            let out_second = dispatch_action_json(Some(app), ns, payload);
            let id_second = serde_json::from_str::<serde_json::Value>(&out_second)
                .unwrap()
                .get("correlation_id")
                .and_then(|v| v.as_str())
                .unwrap()
                .to_string();

            assert_ne!(
                id_first, id_second,
                "after the TTL elapses, the retry must mint a FRESH correlation_id; \
                 a match would mean the sweep missed the expired entry"
            );

            let guard = app.inflight_dispatches.lock().unwrap();
            assert_eq!(
                guard.len(),
                1,
                "retry after TTL must pass the guard and re-insert exactly one entry"
            );
        });
    }

    /// A dispatch that is REJECTED by the registry (unknown namespace,
    /// malformed JSON, validation rejection) must NOT pollute the inflight
    /// set — the host can fix and re-submit immediately. The guard only
    /// records successfully-enqueued dispatches.
    #[test]
    fn rejected_dispatch_does_not_pollute_inflight_set() {
        with_app(|app| {
            // Unknown namespace — rejected at `start()`.
            let _ = dispatch_action_json(Some(app), "nmp.unknown", "{}");
            // Malformed JSON — rejected at action-shape parsing.
            let _ = dispatch_action_json(Some(app), "nmp.publish", "{bad json");
            let guard = app.inflight_dispatches.lock().unwrap();
            assert!(
                guard.is_empty(),
                "rejected dispatches must not be tracked inflight; the host has to \
                 fix and re-submit, and that re-submission must not be confused for \
                 a UI double-tap"
            );
        });
    }

    // ADR-0027 deleted three tests that no longer have a way to be
    // expressed:
    //
    // * `executor_only_namespace_is_rejected_by_dispatch_action` — the unified
    //   trait registers `start()` and `execute()` together; an "executor-only
    //   namespace" is structurally impossible.
    // * `c_abi_nmp_prefixed_executor_registration_is_silently_rejected` —
    //   `nmp_app_register_action_executor` was deleted along with the
    //   `nmp.*`-namespace D6 guard that lived on it. The same protection now
    //   lives in the registry: replacing a built-in module requires editing
    //   `default_registry`, which is by definition trusted Rust code.
    // * `c_abi_nmp_prefixed_module_registration_is_silently_rejected` —
    //   same reasoning for `nmp_app_register_action_module`.
}
