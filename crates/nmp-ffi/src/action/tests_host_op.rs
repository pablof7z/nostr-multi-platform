//! Host-op seam end-to-end tests — split out of `action/tests.rs`
//! (file-size discipline). These exercise `NmpApp::set_host_op_handler` +
//! `ActorCommand::Protocol(HostOpCommand)` (ADR-0052 §D4, K2 rung 5.4),
//! INCLUDING the rung-5.4 panic-isolation oracle
//! (`host_op_panicking_handler_is_isolated_and_actor_survives`). Behaviour
//! is unchanged from when this lived in `tests.rs`; only the file boundary
//! moved.

use std::sync::{Arc, Mutex};

use super::super::{nmp_app_free, nmp_app_new, nmp_app_start};
use super::*;

// ──────────────────────────────────────────────────────────────────
//                HostOpHandler + Protocol/HostOpCommand end-to-end
// ──────────────────────────────────────────────────────────────────
//
// The substrate-generic host-op seam (`NmpApp::set_host_op_handler` +
// `ActorCommand::Protocol(HostOpCommand)`) is the architecturally-correct
// path for stateful, app-owned ops (today: `nmp-app-marmot`'s MLS service)
// through the `dispatch_action` seam. ADR-0025 named the legacy bespoke
// `nmp_marmot_dispatch` FFI cluster as a temporary exception; that
// cluster was deleted in ADR-0025 PR 3 (2026-05-23) and this seam is
// now the SOLE host entry point for stateful host-op dispatch. ADR-0052
// §D4 (K2 rung 5.4) then merged the bespoke `ActorCommand::DispatchHostOp`
// arm into the single `Protocol` write seam as `HostOpCommand`.
//
// End-to-end shape proved here:
//
//   1. An app crate's `ActionModule::execute` body serializes its
//      typed action to JSON and emits `ActorCommand::Protocol(Box::new(
//      host_op_command(action_json, correlation_id)))`.
//   2. The `Protocol` dispatch arm runs the `HostOpCommand`, which clones
//      the host-installed `HostOpHandler` out of the per-app slot (via the
//      `HostOpHandlerAccess` capability) and calls
//      `handle(action_json, correlation_id)`.
//   3. The handler's return value (`{"ok":true,...}` or
//      `{"ok":false,"error":...}`) is routed into the existing
//      `action_stages` / `action_results` mirror via re-entering
//      `RecordActionSuccess` / `RecordActionFailure`.
//   4. A host whose spinner is keyed on the dispatch-returned
//      `correlation_id` sees the terminal verdict on the next
//      snapshot tick — exactly the contract the existing
//      `PublishModule` async-completing path delivers.

/// A test-only `ActionModule` whose `execute` body emits
/// `ActorCommand::Protocol(HostOpCommand)` carrying the action JSON. This is
/// exactly the shape `nmp-app-marmot`'s real `MarmotActionModule`
/// uses — the handler is the only D0-naming piece, kept in the
/// app crate.
struct TestHostOpModule;
impl nmp_core::substrate::ActionModule for TestHostOpModule {
    const NAMESPACE: &'static str = "test.host_op"; // doctrine-allow: D9 — test-only namespace inside #[cfg(test)]; never on the wire
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }
    /// Mirrors the `MarmotActionModule::execute` pattern: serialize the
    /// typed action to JSON and hand it to the `HostOpCommand` on the
    /// `Protocol` arm. No state access — the handler owns that.
    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        let action_json = serde_json::to_string(&action).map_err(|e| e.to_string())?;
        // ADR-0052 §D4 (K2 rung 5.4): host-op dispatch flows through the single
        // `Protocol` write seam as `HostOpCommand` (the bespoke `DispatchHostOp`
        // arm was deleted). The handler still lives in the per-app
        // `HostOpHandlerSlot` set via `NmpApp::set_host_op_handler`.
        send(nmp_core::ActorCommand::Protocol(Box::new(
            nmp_core::substrate::host_op_command(action_json, correlation_id.to_string()),
        )));
        Ok(())
    }
}

/// A stub `HostOpHandler` that records every call so the test can assert
/// the handler was invoked with the same `action_json` and
/// `correlation_id` the dispatcher produced.
struct RecordingHostHandler {
    seen: Arc<Mutex<Vec<(String, String)>>>,
    respond_ok: bool,
}
impl nmp_core::substrate::HostOpHandler for RecordingHostHandler {
    fn handle(&self, action_json: &str, correlation_id: &str) -> serde_json::Value {
        self.seen
            .lock()
            .unwrap()
            .push((action_json.to_string(), correlation_id.to_string()));
        if self.respond_ok {
            serde_json::json!({ "ok": true, "echoed": action_json })
        } else {
            serde_json::json!({ "ok": false, "error": "handler said no" })
        }
    }
}

/// PR 1 end-to-end PROOF — the substrate-generic seam works.
///
/// Host wires `HostOpHandler` and registers `TestHostOpModule`, then
/// `nmp_app_dispatch_action("test.host_op", json)` returns a
/// `correlation_id`. The actor's `DispatchHostOp` arm eventually pulls
/// the handler from the slot and calls `handle` with the same
/// `action_json` payload and the registry-minted `correlation_id`.
///
/// The handler-call observation is a wall-clock poll (≤ 2 s) of the
/// shared `seen` vec rather than a sleep+assert: the actor runs on
/// its own thread, so the dispatch-arm execution is not synchronous
/// with the FFI return. The same shape `host_registered_executor_*`
/// tests use for command-queue observations.
#[test]
fn dispatch_host_op_routes_action_json_to_installed_handler() {
    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(RecordingHostHandler {
        seen: Arc::clone(&seen),
        respond_ok: true,
    });

    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null; valid until `nmp_app_free` below.
    let app_mut = unsafe { &mut *app };

    // Install the substrate-generic handler BEFORE dispatching — the
    // production order is: host init wires both the handler AND the
    // module before any `nmp_app_dispatch_action` arrives.
    app_mut.set_host_op_handler(handler as Arc<dyn nmp_core::substrate::HostOpHandler>);
    let _ = app_mut.register_action(TestHostOpModule);
    nmp_app_start(app, 256, 4);

    let out = dispatch_action_json(
        // SAFETY: `nmp_app_new` never returns null.
        Some(&*app_mut),
        "test.host_op",
        r#"{"op":"create_group","name":"engineering"}"#,
    );
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let returned_id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got: {out}"))
        .to_string();

    // Poll the shared `seen` vec under a 2 s wall-clock cap. The actor
    // dequeues on its own thread; this is the same pattern other
    // command-routing FFI tests use to observe an actor side-effect.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut observed: Option<(String, String)> = None;
    while std::time::Instant::now() < deadline {
        if let Some(entry) = seen.lock().unwrap().first().cloned() {
            observed = Some(entry);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (action_json, correlation_id) = observed.unwrap_or_else(|| {
        panic!(
            "HostOpHandler::handle was never invoked within 2 s of dispatch \
             (returned correlation_id={returned_id})"
        )
    });
    // The handler received the exact action JSON the module's `execute`
    // body produced — the dispatch loop did not mutate the payload.
    let action_value: serde_json::Value = serde_json::from_str(&action_json).unwrap();
    assert_eq!(
        action_value.get("op").and_then(|v| v.as_str()),
        Some("create_group")
    );
    assert_eq!(
        action_value.get("name").and_then(|v| v.as_str()),
        Some("engineering")
    );
    // The registry-minted correlation id matches the value the host
    // received from `dispatch_action` — the spinner-key contract holds.
    assert_eq!(
        correlation_id, returned_id,
        "handler must receive the same correlation_id dispatch_action returned"
    );

    nmp_app_free(app);
}

/// When no handler is installed, a `DispatchHostOp` command does NOT
/// silently drop on the floor — the actor's arm records a `Failed`
/// terminal stage so a host's spinner clears (instead of hanging
/// forever) on the next snapshot tick. This is the D6 "no silent
/// drops" guarantee the dispatch surface relies on.
///
/// The witness is the kernel's `action_stages` projection (read via
/// the snapshot path) carrying a `Failed { reason: "no host op
/// handler installed" }` entry for the dispatched correlation_id.
/// Asserting against the full snapshot is heavy for a unit test, so
/// we instead exercise the dispatch + register path and rely on the
/// happy-path test above to prove the routing — this test confirms
/// only that the dispatch envelope is well-formed (host gets a
/// `correlation_id`) when the handler is absent.
#[test]
fn dispatch_host_op_without_handler_still_returns_correlation_id() {
    let app = nmp_app_new();
    // SAFETY: see the happy-path test.
    let app_mut = unsafe { &mut *app };
    // Register the module WITHOUT installing a handler — the dispatch
    // arm itself records the Failed terminal; the FFI return is still
    // a normal `correlation_id` envelope because `start()` and the
    // `execute()` enqueue both succeed.
    let _ = app_mut.register_action(TestHostOpModule);
    nmp_app_start(app, 256, 4);

    let out = dispatch_action_json(Some(&*app_mut), "test.host_op", r#"{"op":"ping"}"#);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id even without handler, got: {out}"));
    assert_eq!(id.len(), 32, "correlation_id should be 32 hex chars");

    nmp_app_free(app);
}

/// A handler returning `{"ok": false, "error": "..."}` is routed to
/// `record_action_failure` (not `record_action_success`). This is
/// the soft-failure path the Marmot envelope already uses
/// (`{"ok":false,"error":"key_package_unavailable",...}` is a real
/// in-the-wild response).
///
/// We exercise the dispatch + handler-invocation half here; the
/// kernel-side action_stages mirror writes are unit-tested in
/// `kernel/action_stages_tests.rs`. The witness is: the handler was
/// called AND the dispatcher returned a `correlation_id` envelope
/// (not an `error` envelope) — `ok:false` from the handler is a
/// terminal verdict, not a `start()` rejection.
#[test]
fn dispatch_host_op_routes_handler_failure_through_terminal_path() {
    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(RecordingHostHandler {
        seen: Arc::clone(&seen),
        respond_ok: false,
    });

    let app = nmp_app_new();
    // SAFETY: see the happy-path test.
    let app_mut = unsafe { &mut *app };
    app_mut.set_host_op_handler(handler as Arc<dyn nmp_core::substrate::HostOpHandler>);
    let _ = app_mut.register_action(TestHostOpModule);
    nmp_app_start(app, 256, 4);

    let out = dispatch_action_json(Some(&*app_mut), "test.host_op", r#"{"op":"ping"}"#);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        parsed.get("correlation_id").is_some(),
        "soft-failure from a handler is a terminal verdict, NOT a start() rejection — \
         the envelope must still carry a correlation_id; got: {out}"
    );

    // The handler IS reached (same 2 s deadline as the happy-path test).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if !seen.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "handler must have been invoked exactly once"
    );

    nmp_app_free(app);
}

/// A `HostOpHandler` whose `handle` records the call (so we can witness it
/// ran) and then PANICS. The panic must NOT unwind the actor thread.
struct PanickingHostHandler {
    seen: Arc<Mutex<Vec<String>>>,
}
impl nmp_core::substrate::HostOpHandler for PanickingHostHandler {
    fn handle(&self, action_json: &str, _correlation_id: &str) -> serde_json::Value {
        self.seen.lock().unwrap().push(action_json.to_string());
        panic!("host op handler intentionally exploded");
    }
}

/// ADR-0052 §D4 ORACLE (K2 rung 5.4) — the panicking-handler behavioural
/// test. After the `DispatchHostOp` arm was merged into the single `Protocol`
/// seam, a host op whose handler PANICS must still be panic-isolated: the
/// panic is converted (whole-body `catch_unwind` on the `Protocol` arm +
/// `HostOpCommand`'s internal handler catch) instead of unwinding the actor
/// thread, AND the actor SURVIVES — a subsequent dispatch is still processed.
///
/// Witness of survival: install a panicking handler before start, dispatch (the
/// handler is reached and explodes), then dispatch again through the same
/// snapped handler. If the first panic had unwound/killed the actor thread, the
/// second dispatch's handler would never be invoked. We poll (≤ 2 s, the same
/// wall-clock pattern the sibling host-op tests use) for the second invocation
/// — its presence proves the actor outlived the panic.
///
/// FAIL-BEFORE: with the bare `cmd.run(&mut pctx)` the `Protocol` arm had
/// before this rung, a panicking handler reached through the new `HostOpCommand`
/// would unwind the actor thread; the second dispatch's handler would never
/// fire and this test would hang to its 2 s deadline and fail.
#[test]
fn host_op_panicking_handler_is_isolated_and_actor_survives() {
    let panic_seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null; valid until `nmp_app_free`.
    let app_mut = unsafe { &mut *app };
    let _ = app_mut.register_action(TestHostOpModule);

    // 1) Install the PANICKING handler and dispatch — the handler runs and
    //    explodes on the actor thread.
    app_mut.set_host_op_handler(Arc::new(PanickingHostHandler {
        seen: Arc::clone(&panic_seen),
    }) as Arc<dyn nmp_core::substrate::HostOpHandler>);
    nmp_app_start(app, 256, 4);
    let _ = dispatch_action_json(Some(&*app_mut), "test.host_op", r#"{"op":"boom"}"#);

    // The panicking handler IS reached (≤ 2 s wall-clock poll).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if !panic_seen.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        panic_seen.lock().unwrap().len(),
        1,
        "panicking handler must have been invoked before it panicked"
    );

    // 2) Dispatch AGAIN through the same snapped handler. If the panic had
    //    killed the actor thread this dispatch's handler would never fire.
    let _ = dispatch_action_json(Some(&*app_mut), "test.host_op", r#"{"op":"after_panic"}"#);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if panic_seen.lock().unwrap().len() >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let observed = panic_seen.lock().unwrap().get(1).cloned();
    let action_json = observed.expect(
        "actor must SURVIVE the panicking handler — the post-panic dispatch's \
         handler was never invoked within 2 s, so the actor thread died",
    );
    let parsed: serde_json::Value = serde_json::from_str(&action_json).unwrap();
    assert_eq!(
        parsed.get("op").and_then(|v| v.as_str()),
        Some("after_panic"),
        "the surviving actor processed the post-panic op"
    );

    nmp_app_free(app);
}
