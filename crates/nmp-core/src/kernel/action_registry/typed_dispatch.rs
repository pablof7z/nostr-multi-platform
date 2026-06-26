//! Typed-bytes (ADR-0064 / S3 #1751) dispatch arm of [`super::ActionRegistry`].
//!
//! The JSON `start` / `execute` doorway (and the rest of the orchestrator) live
//! in `action_registry.rs`; this sibling carries the typed FlatBuffers twin
//! (`start_bytes` / `execute_bytes`) so neither file exceeds the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). It is a size-management seam, not
//! an API boundary — `start_bytes` / `execute_bytes` are ordinary public methods
//! of `ActionRegistry`.
//!
//! Both methods reach the registered module's typed payload decode through the
//! SOLE typed-decode site, the `ActionModuleAdapter` (`erased.rs`): the adapter
//! calls `ActionModule::decode_payload`, which delegates to the module's
//! `ActionPayload` impl and runs the fail-closed `schema_version` gate BEFORE
//! `start()`. This file never decodes a payload itself.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::erased::TypedDispatchError;
use super::{ActionExecuteFailure, ActionFailureKind, ActionRegistry, new_action_id};
use crate::substrate::{ActionContext, ActionId, ActionRejection};

// `self.modules` is a private field of `ActionRegistry`; this sibling module is
// a child of `action_registry`, so it reaches it directly (no accessor needed).

impl ActionRegistry {
    /// Sorted namespaces currently registered in this action registry.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn action_namespaces(&self) -> Vec<String> {
        let mut out: Vec<String> = self.modules.keys().cloned().collect();
        out.sort();
        out
    }

    /// Typed-bytes twin of [`ActionRegistry::start`]: validate the OPAQUE
    /// per-crate FlatBuffers `payload` against the module registered under
    /// `namespace`, returning the minted `correlation_id`.
    ///
    /// The decode + the fail-closed `schema_version` gate run in the adapter
    /// (the ONE typed-decode site) BEFORE the module's `start()`. Failures are
    /// data ([`ActionRejection`], D6):
    /// * unknown namespace → [`ActionRejection::Invalid`];
    /// * the namespace's module has not migrated to a typed payload →
    ///   [`ActionRejection::Invalid`] (`NotTypedCapable`);
    /// * a `schema_version` trip or malformed buffer →
    ///   [`ActionRejection::Invalid`] carrying the RAW reason (`start()` never
    ///   ran — fail closed);
    /// * `start()` rejected the decoded action → its own [`ActionRejection`].
    ///
    /// The `correlation_id` is minted only after validation succeeds and is the
    /// operation's sole identity end-to-end — never the event id.
    pub fn start_bytes(
        &self,
        ctx: &mut ActionContext,
        now_ms: u64,
        namespace: &str,
        payload: &[u8],
    ) -> Result<ActionId, ActionRejection> {
        let module = self.modules.get(namespace).ok_or_else(|| {
            ActionRejection::Invalid(format!("unknown action namespace: {namespace}"))
        })?;
        // D6: the typed decode + `M::start` body runs on an `extern "C"` call
        // path. A caught panic surfaces as `ActionRejection::Invalid` rather
        // than unwinding across the FFI boundary.
        match catch_unwind(AssertUnwindSafe(|| module.start_bytes(ctx, payload))) {
            Ok(Ok(())) => Ok(new_action_id(now_ms)),
            Ok(Err(TypedDispatchError::Rejected(rejection))) => Err(rejection),
            // Decode / not-typed-capable failures fail closed as Invalid carrying
            // the RAW reason; the module never saw the action.
            Ok(Err(other)) => Err(ActionRejection::Invalid(other.to_string())),
            Err(_) => Err(ActionRejection::Invalid(
                "action validator panicked".to_string(),
            )),
        }
    }

    /// Typed-bytes twin of [`ActionRegistry::execute`]: decode the OPAQUE
    /// `payload` and drive the validated action to the actor via the registered
    /// module's `execute()`.
    ///
    /// Same [`ActionExecuteFailure`] taxonomy + `enqueued`-flag contract as
    /// [`ActionRegistry::execute`] (#1676 BUG-B). The typed decode runs in the
    /// adapter; a decode/not-typed failure maps to
    /// [`ActionFailureKind::SyncError`] with `enqueued: false` (the module never
    /// ran). The call is wrapped in [`catch_unwind`] for the same FFI-boundary
    /// reason as [`ActionRegistry::execute`].
    pub fn execute_bytes(
        &self,
        ctx: &ActionContext,
        namespace: &str,
        payload: &[u8],
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
        let enqueued = std::cell::Cell::new(false);
        let tracking_send = |cmd: crate::actor::ActorCommand| {
            enqueued.set(true);
            send(cmd);
        };
        match catch_unwind(AssertUnwindSafe(|| {
            module.execute_bytes(ctx, payload, correlation_id, &tracking_send)
        })) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => {
                debug_assert!(
                    !enqueued.get(),
                    "ActionModule under namespace '{namespace}' returned Err from \
                     execute_bytes after enqueuing an ActorCommand — violates the \
                     'execute Err ⇒ nothing enqueued' invariant (#1676 BUG-B)"
                );
                Err(ActionExecuteFailure {
                    kind: ActionFailureKind::SyncError,
                    message: err.to_string(),
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
