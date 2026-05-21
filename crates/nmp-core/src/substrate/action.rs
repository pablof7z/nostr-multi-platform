use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub type ActionId = String;

#[derive(Clone, Debug, Default)]
pub struct ActionContext {
    pub now_ms: u64,
}

pub trait ActionModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;

    type Action: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Step: Clone + Serialize + DeserializeOwned + Send + 'static;

    fn start(
        ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection>;

    /// Optional: suggest the correlation_id the registry should assign to
    /// this action instead of the auto-generated one. Returning `Some(id)`
    /// makes `dispatch_action`'s return value and `last_action_result` in the
    /// snapshot use the same identifier — a requirement for hosts that key
    /// spinners on the returned id.
    ///
    /// Default: `None` — the registry generates a unique 32-hex id.
    ///
    /// Override when the action's natural identity is already a stable,
    /// collision-free string visible to the engine (e.g. the pre-signed
    /// event's `id` for `PublishAction::Publish`).
    fn preferred_action_id(_action: &Self::Action) -> Option<ActionId> {
        None
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ActionPlan<Step> {
    pub initial_step: Step,
    pub initial_status: ActionStatus,
    pub deadline_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ActionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ActionRejection {
    Invalid(String),
    Unauthorized(String),
    Conflict(String),
}

/// Delivered to a registered result observer when an action has been
/// **accepted by the registry and enqueued** for execution.
///
/// This is a *push* "action accepted" signal, NOT a completion carrier.
/// Delivery happens after [`crate::kernel::ActionRegistry`]'s `execute`
/// returns `Ok` — i.e. once the action's [`crate::actor::ActorCommand`] has
/// been queued. For an action like `nmp.publish` the actor still has to
/// verify and publish the event after this fires; that eventual outcome is
/// reported through the snapshot-projection (pull) path, not this channel.
///
/// Built-in executors are fire-and-forget and deliver `result_json: null`.
/// A host executor that needs to return a value to the caller writes that
/// value into a snapshot projection (the pull model); `ActionResult` then
/// stays a uniform "accepted" signal, consistent with the single-actor model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActionResult {
    pub correlation_id: String,
    /// JSON-encoded result value, or `null` for fire-and-forget actions.
    pub result_json: serde_json::Value,
}
