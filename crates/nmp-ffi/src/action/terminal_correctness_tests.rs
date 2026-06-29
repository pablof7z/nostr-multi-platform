//! #1676 BUG-A regression at the FFI dispatch seam — the failure fan-in is
//! suppressed when the executor already enqueued a command.
//!
//! New file (not appended to `tests.rs`) because that file already sits at the
//! file-size baseline; the gate rejects growing an over-cap file.
//!
//! Scenario: an async-completing executor sends a real `ActorCommand` and THEN
//! panics. The enqueued command owns the action's terminal verdict, so
//! `dispatch_action_json` must NOT also fan in a `RecordActionFailure` — that
//! would record a SECOND terminal under one correlation_id (the double-terminal
//! bug). The dispatch instead reports the action as accepted-and-enqueued.

use super::super::{test_app_free, test_app_new};
use super::*;

use nmp_core::actor::ActionLedgerCommand;
use nmp_core::substrate::ActionModule;

/// A module that enqueues a terminal-bearing command and then panics — the
/// exact BUG-A shape (an async-completing executor that fails *after* sending).
struct EnqueueThenPanicModule;
impl ActionModule for EnqueueThenPanicModule {
    const NAMESPACE: &'static str = "test.enqueue_then_panic"; // doctrine-allow: action_namespace — test-only namespace inside #[cfg(test)]; never on the wire
    type Action = serde_json::Value;
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }
    fn is_async_completing() -> bool {
        true
    } // doctrine-allow: D12 — test module; the enqueued RecordActionSuccess carries the terminal, asserted here, not via a stage recorded in this file
    fn execute(
        &self,
        _ctx: &ActionContext,
        _action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(nmp_core::actor::ActorCommand),
    ) -> Result<(), String> {
        // Enqueue the real terminal-bearing command, then panic.
        send(nmp_core::actor::ActorCommand::ActionLedger(
            ActionLedgerCommand::RecordSuccess {
                correlation_id: correlation_id.to_string(),
                result_json: None,
            },
        ));
        panic!("module panicked after enqueueing");
    }
}

#[test]
fn async_completing_executor_enqueue_then_panic_suppresses_failure_fanin() {
    let app = test_app_new();
    // SAFETY: `test_app_new` never returns null; valid until `test_app_free`.
    let app_mut = unsafe { &mut *app };
    let _ = app_mut.register_action(EnqueueThenPanicModule);

    // The monotone send counter only ever increments, so reading it after the
    // synchronous `dispatch_action_json` returns is race-free.
    let sends_before = app_mut.send_cmd_count_for_test();
    let out = dispatch_action_json(Some(&*app_mut), "test.enqueue_then_panic", "{}");
    let sends_after = app_mut.send_cmd_count_for_test();

    let parsed: serde_json::Value =
        serde_json::from_str(&out).expect("dispatch envelope must be parseable JSON");

    // Accepted envelope: the enqueued command owns the terminal, so the
    // dispatch reports accepted (correlation_id present) with NO error field.
    assert!(
        parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .is_some(),
        "envelope must carry the correlation_id; got: {out}"
    );
    assert!(
        parsed.get("error").is_none(),
        "no error: the enqueued command owns the terminal, fan-in suppressed; got: {out}"
    );

    // Exactly ONE ActorCommand was sent: the executor's enqueue. A SECOND send
    // would be the spurious `RecordActionFailure` fan-in — the BUG-A double
    // terminal. Suppression means the delta is exactly one.
    assert_eq!(
        sends_after - sends_before,
        1,
        "exactly one ActorCommand (the enqueue) — the failure fan-in must be \
         suppressed so no second terminal lands under this correlation_id; \
         sends_before={sends_before} sends_after={sends_after}"
    );

    test_app_free(app);
}
