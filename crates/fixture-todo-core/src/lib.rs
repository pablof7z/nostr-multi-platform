use nmp_core::substrate::*;
use serde::{Deserialize, Serialize};

pub const APP_MODULE: &str = "fixture.todo";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TodoRecord {
    pub id: String,
    pub title: String,
    pub completed: bool,
}

pub struct TodoDomainModule;

impl DomainModule for TodoDomainModule {
    const NAMESPACE: &'static str = "fixture.todo.domain";
    const SCHEMA_VERSION: u32 = 1;

    fn migrations() -> Vec<DomainMigration> {
        Vec::new()
    }

    fn indexes() -> Vec<DomainIndex> {
        vec![DomainIndex {
            name: "by_completed",
            key_fn: |bytes| {
                serde_json::from_slice::<TodoRecord>(bytes)
                    .ok()
                    .map(|todo| todo.completed.to_string().into_bytes())
            },
        }]
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TodoListSpec {
    pub include_completed: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TodoListView {
    pub items: Vec<TodoRecord>,
    pub open_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TodoDelta {
    Replaced { payload: TodoListView },
}

#[derive(Clone, Debug, Default)]
pub struct TodoViewState {
    payload: TodoListView,
}

pub struct TodoViewModule;

impl ViewModule for TodoViewModule {
    const NAMESPACE: &'static str = "fixture.todo.view";

    type Spec = TodoListSpec;
    type Payload = TodoListView;
    type Delta = TodoDelta;
    type Key = bool;
    type State = TodoViewState;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.include_completed
    }

    fn dependencies(_spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies::default()
    }

    fn open(_ctx: &ViewContext, _spec: Self::Spec) -> (Self::State, Self::Payload) {
        let payload = TodoListView::default();
        (
            TodoViewState {
                payload: payload.clone(),
            },
            payload,
        )
    }

    fn on_event_inserted(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _event: &KernelEvent,
    ) -> Option<Self::Delta> {
        None
    }

    fn on_event_removed(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _id: &EventId,
    ) -> Option<Self::Delta> {
        None
    }

    fn on_event_replaced(
        _ctx: &ViewContext,
        _state: &mut Self::State,
        _old_id: &EventId,
        _new_event: &KernelEvent,
    ) -> Option<Self::Delta> {
        None
    }

    fn on_projection_changed(
        _ctx: &ViewContext,
        state: &mut Self::State,
        _change: &ProjectionChange,
    ) -> Option<Self::Delta> {
        Some(TodoDelta::Replaced {
            payload: state.payload.clone(),
        })
    }

    fn snapshot(_ctx: &ViewContext, state: &Self::State) -> Self::Payload {
        state.payload.clone()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Action {
    Add { id: String, title: String },
    Toggle { id: String },
    ClearCompleted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TodoStep {
    ApplyLocalWrite,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ActionOutput {
    Accepted,
}

pub struct TodoActionModule;

impl ActionModule for TodoActionModule {
    const NAMESPACE: &'static str = "fixture.todo.action";

    type Action = Action;
    type Step = TodoStep;
    type Output = ActionOutput;

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        if matches!(&action, Action::Add { title, .. } if title.trim().is_empty()) {
            return Err(ActionRejection::Invalid("todo title is empty".to_string()));
        }
        Ok(ActionPlan {
            initial_step: TodoStep::ApplyLocalWrite,
            initial_status: ActionStatus::Running,
            deadline_ms: None,
        })
    }

    fn reduce(
        _ctx: &mut ActionContext,
        _id: ActionId,
        _input: ActionInput<Self::Step>,
    ) -> ActionTransition<Self::Step, Self::Output> {
        ActionTransition::Complete {
            output: ActionOutput::Accepted,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CapabilityCall {
    CountOpenTodos,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityResult {
    pub count: usize,
}

pub struct TodoCapabilityModule;

impl CapabilityModule for TodoCapabilityModule {
    const NAMESPACE: &'static str = "fixture.todo.capability";

    type Request = CapabilityCall;
    type Result = CapabilityResult;

    fn callback_interface_name() -> &'static str {
        "FixtureTodoCapability"
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TodoIdentityDescriptor {
    pub label: String,
}

pub struct TodoIdentityModule;

impl IdentityModule for TodoIdentityModule {
    const NAMESPACE: &'static str = "fixture.todo.identity";

    type Descriptor = TodoIdentityDescriptor;

    fn scope_kind() -> IdentityScopeKind {
        IdentityScopeKind::AppLocal
    }

    fn create(
        ctx: &mut IdentityContext,
        descriptor: Self::Descriptor,
    ) -> Result<IdentityId, IdentityError> {
        if descriptor.label.trim().is_empty() {
            return Err(IdentityError::InvalidDescriptor("empty label".to_string()));
        }
        let id = format!("fixture-todo:{}", descriptor.label);
        ctx.remember(id.clone());
        Ok(id)
    }

    fn sign<'a>(
        _ctx: &'a IdentityContext,
        _id: &'a IdentityId,
        _unsigned: &'a UnsignedEvent,
    ) -> BoxFuture<'a, Result<SignedEvent, SigningError>> {
        Box::pin(async {
            Err(SigningError::Unsupported(
                "fixture identity does not sign Nostr events".to_string(),
            ))
        })
    }

    fn destroy(_ctx: &mut IdentityContext, _id: &IdentityId) {}
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ViewSpec {
    TodoList(TodoListSpec),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Update {
    TodoList(TodoListView),
    ActionAccepted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_rejects_empty_todo_title() {
        let mut ctx = ActionContext::default();
        let result = TodoActionModule::start(
            &mut ctx,
            Action::Add {
                id: "1".to_string(),
                title: " ".to_string(),
            },
        );

        assert_eq!(
            result,
            Err(ActionRejection::Invalid("todo title is empty".to_string()))
        );
    }
}
