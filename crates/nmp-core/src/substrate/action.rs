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
    type Output: Clone + Serialize + Send + 'static;

    fn start(
        ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection>;

    fn reduce(
        ctx: &mut ActionContext,
        id: ActionId,
        input: ActionInput<Self::Step>,
    ) -> ActionTransition<Self::Step, Self::Output>;
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ActionInput<Step> {
    Started,
    ResumedAfterRestart { step: Step },
    CapabilityResult { value: serde_json::Value },
    RelayOk { relay_url: String },
    Timeout,
    Cancel,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ActionTransition<Step, Output> {
    Continue {
        step: Step,
        status: ActionStatus,
    },
    Complete {
        output: Output,
    },
    Fail {
        reason: String,
        transient: bool,
    },
    AwaitCapability {
        request_namespace: String,
        payload: serde_json::Value,
        next_step: Step,
    },
    AwaitUserApproval {
        prompt: String,
        next_step: Step,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ActionRejection {
    Invalid(String),
    Unauthorized(String),
    Conflict(String),
}
