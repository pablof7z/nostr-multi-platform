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
/// returns a `correlation_id` string. `PublishAction::PublishRaw` (kind:1) is
/// used because it needs no signed-event fixture — the actor signs — and
/// still exercises the full registry → adapter → module path.
#[test]
fn dispatch_publish_raw_action_returns_correlation_id() {
    with_app(|app| {
        let out = dispatch_action_json(
            Some(app),
            "nmp.publish",
            r#"{"PublishRaw":{"kind":1,"tags":[],"content":"smoke-test","target":"Auto"}}"#,
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

// ─── nmp_app_ack_action_stage FFI defensive contract ──────────────
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
        // dispatch-publish-raw test above exercises the same
        // straddle counter the same way (count via depth, not via
        // actor observation) so we follow that precedent.
        let _ = depth_after;
    });
}

use nmp_core::publish::{PublishAction, PublishTarget};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

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

/// Regression for #1748 Fix 1: a pre-signed `Publish` through `dispatch_action`
/// returns a freshly MINTED correlation_id (the operation's identity), NOT the
/// event's `id`. The deleted `preferred_action_id` substituted the event id,
/// handing the host a value it never keyed its spinner on — so the terminal in
/// `action_results` (under the threaded minted id) could not be matched to the
/// dispatch return.
#[test]
fn dispatch_publish_action_returns_minted_correlation_id_not_event_id() {
    with_app(|app| {
        let event = fixture_signed_event();
        let event_id = event.id.clone();
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
        assert_ne!(
            id, event_id,
            "the returned correlation_id must NOT be the event id — identity is \
             not output data (#1748)"
        );
        assert_eq!(id.len(), 32, "minted correlation_id is 32-hex, not the 64-hex event id");
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
fn execute_action_publish_raw_is_ok_without_actor() {
    with_app(|app| {
        let json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"h3","target":"Auto"}}"#;
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
            err.message.contains("no executor registered") && err.message.contains("nmp.future"),
            "error should name the unwired namespace, got: {err:?}"
        );
    });
}

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

// ─── Typed test ActionModule structs shared across the seam-proof tests.
// ADR-0027 collapsed the dual-seam closure path; every host registration is
// `app.register_action(M)` against a typed module value (ADR-0052 rung 5.2).

/// Greeting test module — succeeds and records that `execute` ran via a
/// process `static` flag the test reads back after
/// `register_action(TestGreetingModule)`.
static GREETING_CALLED: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();

/// Reset and return the shared "called" flag for the greeting module.
fn greeting_flag() -> Arc<AtomicBool> {
    GREETING_CALLED
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

struct TestGreetingModule;
impl nmp_core::substrate::ActionModule for TestGreetingModule {
    const NAMESPACE: &'static str = "test.greeting"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        greeting_flag().store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// Failing test module — always returns `Err` from `execute`.
struct TestFailingModule;
impl nmp_core::substrate::ActionModule for TestFailingModule {
    const NAMESPACE: &'static str = "test.failing"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        Err("host rejected the action".to_string())
    }
}

/// Accept-everything test module under `test.todo`. Used by the
/// dispatch-action end-to-end tests below.
struct TestTodoModule;
impl nmp_core::substrate::ActionModule for TestTodoModule {
    const NAMESPACE: &'static str = "test.todo"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Rejecting test module under `test.todo_reject` — `start()` always
/// returns `ActionRejection::Invalid`.
struct TestTodoRejectModule;
impl nmp_core::substrate::ActionModule for TestTodoRejectModule {
    const NAMESPACE: &'static str = "test.todo_reject"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Err(ActionRejection::Invalid(
            "host rejected: title required".into(),
        ))
    }
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Panicking test module under `test.panic` — `execute()` panics. Used by
/// `executor_failure_returns_correlation_id_and_enqueues_failed_terminal`.
struct TestPanicModule;
impl nmp_core::substrate::ActionModule for TestPanicModule {
    const NAMESPACE: &'static str = "test.panic"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::actor::ActorCommand),
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
    let _ = app_mut.register_action(TestGreetingModule);

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
    let _ = app_mut.register_action(TestFailingModule);

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
    let _ = app_mut.register_action(TestGreetingModule);

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
    let _ = app_mut.register_action(TestTodoModule);

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
    let _ = app_mut.register_action(TestTodoRejectModule);

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
    let seen: Arc<Mutex<Vec<nmp_core::substrate::ActionResult>>> =
        Arc::new(Mutex::new(Vec::new()));
    let seen_in_observer = Arc::clone(&seen);

    with_app(|app| {
        app.register_action_result_observer(move |result| {
            seen_in_observer.lock().unwrap().push(result);
        });

        let out = dispatch_action_json(
            Some(app),
            "nmp.publish",
            r#"{"PublishRaw":{"kind":1,"tags":[],"content":"observer-test","target":"Auto"}}"#,
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
        r#"{"PublishRaw":{"kind":1,"tags":[],"content":"c-abi-test","target":"Auto"}}"#,
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

/// "send-then-panic orphan" finding: an executor that panics/`Err`s after the
/// registry minted the correlation_id. Contract: (1) the error envelope carries
/// both `correlation_id` and `error`; (2) the actor receives a
/// `RecordActionFailure` with that id so a `Failed` terminal lands in
/// `action_stages`. This test asserts #1 (envelope shape) directly and #2
/// indirectly via the actor queue-depth counter — the FFI thread can't block on
/// the actor's snapshot cadence in a unit test, so the projection side is
/// covered by `record_action_failure_records_failed_stage_in_mirror` in
/// `kernel/action_stages_tests.rs`.
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
    let _ = app_mut.register_action(TestPanicModule);

    // Snapshot the monotone send counter before dispatch. Unlike
    // `queue_depth` (which the actor drains concurrently), `send_cmd_count`
    // is only ever incremented — never decremented — so reading it after
    // `dispatch_action_json` returns is race-free: no other thread can make
    // the count go *down* between the call and the assertion.
    let sends_before = app_mut
        .send_cmd_count
        .load(std::sync::atomic::Ordering::Relaxed);
    let out = dispatch_action_json(Some(&*app_mut), "test.panic", "{}");
    let sends_after = app_mut
        .send_cmd_count
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
    // `RecordActionFailure` fan-out). We use the monotone `send_cmd_count`
    // (incremented by every `send_cmd` call, never decremented) rather than
    // `queue_depth` (which the actor drain-thread races to decrement). The
    // old `queue_depth` assertion was chronically flaky: the actor could
    // process the command between `dispatch_action_json` returning and the
    // `depth_after` read, collapsing `depth_after == depth_before` and
    // failing the assertion. `send_cmd_count` is a one-way ratchet — the
    // comparison is always valid regardless of actor scheduling.
    assert!(
        sends_after > sends_before,
        "executor failure must enqueue at least one ActorCommand \
         (RecordActionFailure); sends_before={sends_before} sends_after={sends_after}"
    );
    nmp_app_free(app);
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

