//! JSON execution arm for [`super::ActionRegistry`].
//!
//! Kept beside the typed-byte execution arm so `action_registry.rs` stays below
//! the hand-authored file-size ceiling while the public methods remain on
//! `ActionRegistry`.

use std::panic::{catch_unwind, AssertUnwindSafe};

use super::{ActionExecuteFailure, ActionFailureKind, ActionRegistry};
use crate::substrate::ActionContext;

impl ActionRegistry {
    /// Execute the validated action via [`crate::substrate::ActionModule::execute`]
    /// on the registered module (ADR-0071).
    ///
    /// On failure returns a typed [`ActionExecuteFailure`] carrying the
    /// taxonomy [`kind`](ActionFailureKind) (no-executor / sync-err / panic),
    /// a host-facing message, and the load-bearing `enqueued` flag.
    ///
    /// # Contract (#1676 BUG-B)
    ///
    /// **`execute` returning `Err` ⇒ nothing was enqueued.** A well-behaved
    /// module that sends an [`crate::actor::ActorCommand`] then returns `Ok`
    /// (the async-completing pattern: the enqueued command produces the
    /// terminal verdict asynchronously). A module that enqueues and *then*
    /// fails violates this contract; the returned `enqueued` flag records it so
    /// the dispatch caller can preserve the one-terminal-per-dispatch invariant
    /// (#1676 BUG-A) by suppressing the failure fan-in. A sync `Err` after an
    /// enqueue additionally trips a `debug_assert` (loud in dev/test).
    ///
    /// D6: the call is wrapped in [`catch_unwind`] because the typed
    /// `M::execute` body runs on the `nmp_app_dispatch_action` call path (an
    /// `extern "C"` function) and may include user-supplied (module-author)
    /// code. A caught panic returns an [`ActionFailureKind::Panic`] failure
    /// rather than unwinding across the FFI boundary.
    pub fn execute(
        &self,
        ctx: &ActionContext,
        namespace: &str,
        action_json: &str,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), ActionExecuteFailure> {
        let Some(module) = self.modules.get(namespace) else {
            return Err(ActionExecuteFailure {
                kind: ActionFailureKind::NoExecutor,
                message: format!("no executor registered for namespace '{namespace}'"),
                enqueued: false,
            });
        };
        // Track whether the module sent any `ActorCommand` before completing.
        // `Cell` (not `AtomicBool`): `execute` runs single-threaded on the FFI
        // dispatch thread. The flag survives a panic unwind because it lives on
        // this stack frame, outside the `catch_unwind` closure.
        let enqueued = std::cell::Cell::new(false);
        let tracking_send = |cmd: crate::actor::ActorCommand| {
            enqueued.set(true);
            send(cmd);
        };
        match catch_unwind(AssertUnwindSafe(|| {
            module.execute(ctx, action_json, correlation_id, &tracking_send)
        })) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => {
                // BUG-B invariant: a sync `Err` must not have enqueued. A module
                // that does is buggy — loud in dev/test, fail-safe in release
                // (the `enqueued` flag still suppresses the caller's fan-in so
                // the one-terminal invariant holds).
                debug_assert!(
                    !enqueued.get(),
                    "ActionModule under namespace '{namespace}' returned Err from \
                     execute after enqueuing an ActorCommand — violates the \
                     'execute Err ⇒ nothing enqueued' invariant (#1676 BUG-B)"
                );
                Err(ActionExecuteFailure {
                    kind: ActionFailureKind::SyncError,
                    message,
                    enqueued: enqueued.get(),
                })
            }
            Err(_) => Err(ActionExecuteFailure {
                kind: ActionFailureKind::Panic,
                message: "action executor panicked".to_string(),
                enqueued: enqueued.get(),
            }),
        }
    }
}
