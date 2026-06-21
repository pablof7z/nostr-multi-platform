//! Dyn-erasure layer for [`super::ActionRegistry`].
//!
//! [`ActionModule`] carries an associated `Action` type, so a `HashMap` of
//! trait objects needs a dyn-safe facade. [`ErasedActionModule`] is that facade
//! (it speaks JSON at the boundary) and [`ActionModuleAdapter`] is the sole
//! implementor, round-tripping each module's concrete `Action` through serde.
//!
//! Extracted from `action_registry.rs` to keep that orchestrator file under the
//! 500-LOC hand-authored ceiling (AGENTS.md / V-12).

use crate::substrate::{ActionContext, ActionId, ActionModule, ActionRejection};

/// Dyn-safe facade over [`ActionModule`].
///
/// `ActionModule` carries an associated `Action` type, so it cannot be stored
/// as `Box<dyn ActionModule>` directly. This trait erases it to a JSON string
/// at the boundary so the registry can hold a heterogeneous map of modules.
/// [`ActionModuleAdapter`] is the sole implementor (ADR-0027 deleted the
/// pre-existing `ClosureModule` half); it round-trips each module's typed
/// action shape through serde.
pub(super) trait ErasedActionModule: Send + Sync {
    /// Validate `action_json` against the module's `Action` type and return
    /// an optional preferred correlation id. Mirrors [`ActionModule::start`] +
    /// [`ActionModule::preferred_action_id`].
    ///
    /// `None` preferred id → caller uses [`super::new_action_id`]. `Some(id)` →
    /// caller uses that id directly (e.g. the signed event's `id` field for
    /// `PublishAction::Publish`, so that `dispatch_action`'s return and the
    /// matching `action_results` entry share the same identifier).
    fn start(
        &self,
        ctx: &mut ActionContext,
        action_json: &str,
    ) -> Result<Option<ActionId>, ActionRejection>;

    /// Execute the validated action. Called by [`super::ActionRegistry::execute`]
    /// after `start` returns `Ok`.
    fn execute(
        &self,
        action_json: &str,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String>;
}

/// Adapter binding a concrete [`ActionModule`] `M` to the dyn-safe
/// [`ErasedActionModule`] facade.
///
/// ADR-0052 rung 5.2: the adapter holds the module **by value** (was a ZST
/// `PhantomData<M>`). This lets a stateful module own its dependencies (an
/// `Arc<WalletRuntimeHandle>`, an `Arc<DmRelayCache>`, …) captured by the host
/// at composition time, so `start`/`execute` reach that state through
/// `&self.0` rather than a process-global. Stateless modules are unit-shaped
/// values, so this stays effectively zero-cost for them.
pub(super) struct ActionModuleAdapter<M: ActionModule>(pub(super) M);

impl<M: ActionModule> ErasedActionModule for ActionModuleAdapter<M> {
    fn start(
        &self,
        ctx: &mut ActionContext,
        action_json: &str,
    ) -> Result<Option<ActionId>, ActionRejection> {
        let action: M::Action = serde_json::from_str(action_json)
            .map_err(|e| ActionRejection::Invalid(e.to_string()))?;
        // Query preferred id before moving `action` into `start`.
        let preferred_id = M::preferred_action_id(&action);
        self.0.start(ctx, action)?;
        Ok(preferred_id)
    }

    fn execute(
        &self,
        action_json: &str,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String> {
        let action: M::Action = serde_json::from_str(action_json).map_err(|e| e.to_string())?;
        self.0.execute(action, correlation_id, send)
    }
}
