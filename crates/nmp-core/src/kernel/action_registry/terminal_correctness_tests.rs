//! #1676 — action failure taxonomy + the "execute Err ⇒ nothing enqueued"
//! invariant.
//!
//! New file (not appended to `tests.rs`) because that file already sits at the
//! file-size baseline; the gate rejects growing an over-cap file. These tests
//! pin [`ActionRegistry::execute`]'s typed [`ActionExecuteFailure`] return:
//! the taxonomy [`kind`](ActionFailureKind) distinguishes a no-executor
//! rejection from an intentional sync refusal from a crash (#1676 BUG-B), and
//! the `enqueued` flag is the discriminator the dispatch layer uses to suppress
//! a double terminal (#1676 BUG-A).

use super::*;
use crate::actor::ActionLedgerCommand;
use crate::substrate::ActionContext;
use std::cell::Cell;

fn ctx() -> ActionContext {
    ActionContext::default()
}

/// A no-executor namespace fails with [`ActionFailureKind::NoExecutor`] — a
/// pre-enqueue rejection. Nothing ran, so `enqueued` is false.
#[test]
fn unknown_namespace_execute_is_tagged_no_executor() {
    let registry = ActionRegistry::new();
    let err = registry
        .execute(&ctx(), "host.absent", "null", "corr-id", &|_cmd| {})
        .expect_err("an unregistered namespace must return Err");
    assert_eq!(err.kind, ActionFailureKind::NoExecutor, "got: {err:?}");
    assert!(
        err.message.contains("no executor registered"),
        "got: {err:?}"
    );
    assert!(!err.enqueued, "nothing ran, so nothing enqueued: {err:?}");
}

/// An intentional sync `Err(String)` is tagged [`ActionFailureKind::SyncError`],
/// NOT `Panic`. Before #1676 both collapsed into the same opaque `Err`, so a
/// host could only tell a crash from a refusal by string-matching the panic
/// sentinel. The contract "execute `Err` ⇒ nothing enqueued" holds here: the
/// module sends no command, so `enqueued` is false and the fan-in is safe.
#[test]
fn sync_err_executor_is_tagged_sync_error_not_panic() {
    struct RefusingModule;
    impl ActionModule for RefusingModule {
        const NAMESPACE: crate::substrate::DeclaredActionNamespace =
            crate::substrate::DeclaredActionNamespace::app_owned("host.refuse");
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
            _ctx: &ActionContext,
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            Err("refused: precondition not met".to_string())
        }
    }

    let mut registry = ActionRegistry::new();
    let _ = registry.register(RefusingModule);
    let err = registry
        .execute(&ctx(), "host.refuse", "null", "corr-id", &|_cmd| {})
        .expect_err("a refusing executor must return Err");
    assert_eq!(
        err.kind,
        ActionFailureKind::SyncError,
        "an intentional refusal is SyncError, not Panic: {err:?}"
    );
    assert_eq!(err.message, "refused: precondition not met", "got: {err:?}");
    assert!(
        !err.enqueued,
        "a sync Err must not have enqueued (BUG-B invariant): {err:?}"
    );
}

/// A caught panic is tagged [`ActionFailureKind::Panic`]. A panic that fires
/// *before* any enqueue reports `enqueued == false`, so the dispatch fan-in
/// remains the sole terminal — safe.
#[test]
fn panic_before_enqueue_is_tagged_panic_and_not_enqueued() {
    struct PanicFirstModule;
    impl ActionModule for PanicFirstModule {
        const NAMESPACE: crate::substrate::DeclaredActionNamespace =
            crate::substrate::DeclaredActionNamespace::app_owned("host.panic_first");
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
            _ctx: &ActionContext,
            _action: Self::Action,
            _correlation_id: &str,
            _send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            panic!("crashed before sending");
        }
    }

    let mut registry = ActionRegistry::new();
    let _ = registry.register(PanicFirstModule);
    let err = registry
        .execute(&ctx(), "host.panic_first", "null", "corr-id", &|_cmd| {})
        .expect_err("a panicking executor returns Err, not unwind");
    assert_eq!(err.kind, ActionFailureKind::Panic, "got: {err:?}");
    assert!(
        !err.enqueued,
        "a pre-enqueue panic enqueued nothing: {err:?}"
    );
}

/// #1676 BUG-A core: a module that enqueues an `ActorCommand` and THEN panics
/// reports `enqueued == true`. This is the flag the dispatch layer reads to
/// suppress the failure fan-in (the enqueued command owns the terminal), and
/// proves the registry observes a send that happened before the unwind.
#[test]
fn panic_after_enqueue_reports_enqueued_true() {
    struct EnqueueThenPanicModule;
    impl ActionModule for EnqueueThenPanicModule {
        const NAMESPACE: crate::substrate::DeclaredActionNamespace =
            crate::substrate::DeclaredActionNamespace::app_owned("host.enqueue_then_panic");
        type Action = serde_json::Value;
        fn start(
            &self,
            _ctx: &mut ActionContext,
            _action: Self::Action,
        ) -> Result<(), ActionRejection> {
            Ok(())
        }
        #[rustfmt::skip]
        fn is_async_completing() -> bool { true } // doctrine-allow: D12 — test module; the enqueued command (asserted via `seen`) carries the terminal, not a stage recorded in this file
        fn execute(
            &self,
            _ctx: &ActionContext,
            _action: Self::Action,
            correlation_id: &str,
            send: &dyn Fn(crate::actor::ActorCommand),
        ) -> Result<(), String> {
            send(crate::actor::ActorCommand::ActionLedger(
                ActionLedgerCommand::RecordSuccess {
                    correlation_id: correlation_id.to_string(),
                    result_json: None,
                },
            ));
            panic!("crashed after sending");
        }
    }

    let mut registry = ActionRegistry::new();
    let _ = registry.register(EnqueueThenPanicModule);
    let seen = Cell::new(0u32);
    let err = registry
        .execute(
            &ctx(),
            "host.enqueue_then_panic",
            "null",
            "corr-id",
            &|_cmd| {
                seen.set(seen.get() + 1);
            },
        )
        .expect_err("a post-enqueue panic still returns Err");
    assert_eq!(err.kind, ActionFailureKind::Panic, "got: {err:?}");
    assert_eq!(
        seen.get(),
        1,
        "the module's pre-panic send must reach the host send"
    );
    assert!(
        err.enqueued,
        "a command was enqueued before the panic — the fan-in MUST be suppressed: {err:?}"
    );
}
