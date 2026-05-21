use std::sync::{Arc, Mutex};

use nmp_core::substrate::*;
use nmp_core::NmpApp;
use serde::{Deserialize, Serialize};

pub const APP_MODULE: &str = "fixture.todo";

/// The action namespace the host wires into `NmpApp`'s action registry. A
/// dispatcher names this string in `nmp_app_dispatch_action`; it MUST equal
/// [`TodoActionModule::NAMESPACE`].
///
/// `ACTION_NAMESPACE` is the **codegen convention name**: `nmp-codegen` emits,
/// for every app module, a `dispatch_action(<crate>::ACTION_NAMESPACE, …)`
/// arm — so every app module crate MUST export this exact symbol.
pub const ACTION_NAMESPACE: &str = TodoActionModule::NAMESPACE;

/// The snapshot-projection key the host registers. The current todo list is
/// projected under `KernelSnapshot::projections["fixture.todo.items"]` on
/// every snapshot tick — a non-social host carving out its own snapshot
/// namespace WITHOUT editing `nmp-core`'s sealed `KernelSnapshot` fields.
pub const TODO_SNAPSHOT_KEY: &str = "fixture.todo.items";

/// The host-owned todo store. The registered action executor mutates it; the
/// registered snapshot projector reads it. Shared `Arc<Mutex<…>>` so both the
/// FFI thread (where the executor runs) and the actor thread (where the
/// projector runs on each snapshot tick) observe the same `Vec`.
pub type TodoStore = Arc<Mutex<Vec<TodoRecord>>>;

/// The store-handle type [`register`] returns — the **codegen convention
/// name**. `nmp-codegen` types the generated `FfiApp`'s per-module field as
/// `<crate>::Store`, so every app module crate MUST export this exact alias.
pub type Store = TodoStore;

/// Apply one validated [`Action`] to the host-owned todo `store`.
///
/// This is the fixture's "executor body": `Add` appends (or replaces an
/// existing id), `Toggle` flips `completed`, `ClearCompleted` drops completed
/// records. Pure over the `Vec` — no FFI, no actor command — so the snapshot
/// projector simply reads the result. Shape validation (empty-title rejection)
/// already happened in [`TodoActionModule::start`]; this only mutates state.
pub fn apply_todo_action(store: &mut Vec<TodoRecord>, action: Action) {
    match action {
        Action::Add { id, title } => {
            if let Some(existing) = store.iter_mut().find(|r| r.id == id) {
                existing.title = title;
            } else {
                store.push(TodoRecord {
                    id,
                    title,
                    completed: false,
                });
            }
        }
        Action::Toggle { id } => {
            if let Some(record) = store.iter_mut().find(|r| r.id == id) {
                record.completed = !record.completed;
            }
        }
        Action::ClearCompleted => {
            store.retain(|r| !r.completed);
        }
    }
}

/// Pure projection of the current todo list into the JSON value the host
/// contributes under [`TODO_SNAPSHOT_KEY`].
///
/// Factored out as a free function so the registered snapshot closure stays a
/// one-line delegate AND a unit test can assert the JSON shape without
/// reaching into `nmp-core`'s `pub(super)` `KernelSnapshot::projections`
/// (the end-to-end snapshot-tick path is proven by `nmp-core`'s own
/// `snapshot_registry_tests.rs`).
pub fn project_todo_items(items: &[TodoRecord]) -> serde_json::Value {
    let open_count = items.iter().filter(|r| !r.completed).count();
    serde_json::json!({
        "items": items,
        "open_count": open_count,
    })
}

/// Wire the fixture's todo namespace into `app`'s live extensibility seams and
/// return the shared [`TodoStore`] the host retains.
///
/// `register` is the **codegen convention name**: `nmp-codegen` emits a
/// `<crate>::register(&mut *app)` call in the generated `FfiApp::new` for every
/// app module, so every app module crate MUST export this exact symbol.
///
/// This is the fixture's proof of NMP's host-extensibility thesis — it
/// registers, against a vanilla `NmpApp`, WITHOUT editing `nmp-core`:
///
/// * an **action module** for [`ACTION_NAMESPACE`] — the `start()`
///   validator, delegating to [`TodoActionModule::start`] (the existing
///   empty-title rejection is reused, never reimplemented);
/// * an **action executor** for the same namespace — applies the validated
///   action to the returned store via [`apply_todo_action`]. The todo flow is
///   local-only, so the actor-command `send` bridge is intentionally unused;
/// * a **snapshot projection** under [`TODO_SNAPSHOT_KEY`] — projects the
///   store into JSON via [`project_todo_items`] on every snapshot tick.
///
/// After this call, `nmp_app_dispatch_action(app, ACTION_NAMESPACE, …)`
/// drives a todo action end-to-end: `start()` validates, `execute()` mutates
/// the store, and the next snapshot carries the projected list.
///
/// MUST be called during host init — before `nmp_app_start` and before any
/// `nmp_app_dispatch_action` — because [`NmpApp::register_action_module`] /
/// [`NmpApp::register_action_executor`] take `&mut NmpApp`.
pub fn register(app: &mut NmpApp) -> TodoStore {
    let store: TodoStore = Arc::new(Mutex::new(Vec::new()));

    // Module half — `start()` validation. Decode the action JSON into the
    // typed `Action`, then delegate to the existing `TodoActionModule::start`
    // so the empty-title rejection rule has exactly one home. The kernel keys
    // its registry on `serde_json::Value` steps, so erase the typed `TodoStep`.
    app.register_action_module(ACTION_NAMESPACE, |action_json| {
        let action: Action = serde_json::from_str(action_json)
            .map_err(|e| ActionRejection::Invalid(e.to_string()))?;
        let mut ctx = ActionContext::default();
        let plan = TodoActionModule::start(&mut ctx, action)?;
        Ok(ActionPlan {
            initial_step: serde_json::to_value(&plan.initial_step)
                .unwrap_or(serde_json::Value::Null),
            initial_status: plan.initial_status,
            deadline_ms: plan.deadline_ms,
        })
    });

    // Executor half — `execute()`. Decode the (already-validated) action and
    // apply it to the host-owned store. The todo flow is local-only: no
    // `ActorCommand` is needed, so the `send` bridge is deliberately unused.
    let executor_store = Arc::clone(&store);
    app.register_action_executor(ACTION_NAMESPACE, move |action_json, _correlation_id, _send| {
        let action: Action = serde_json::from_str(action_json).map_err(|e| e.to_string())?;
        let mut guard = executor_store
            .lock()
            .map_err(|_| "todo store mutex poisoned".to_string())?;
        apply_todo_action(&mut guard, action);
        Ok(())
    });

    // Snapshot-output half — projects the store under `TODO_SNAPSHOT_KEY` on
    // every snapshot tick. D8: cheap, non-blocking (one lock + clone).
    let projector_store = Arc::clone(&store);
    app.register_snapshot_projection(TODO_SNAPSHOT_KEY, move || {
        match projector_store.lock() {
            Ok(guard) => project_todo_items(&guard),
            // D6: a poisoned store mutex collapses to JSON `null` rather than
            // panicking inside the actor's snapshot tick.
            Err(_) => serde_json::Value::Null,
        }
    });

    store
}

/// The [`Update`] value the generated `FfiApp::dispatch` returns when a todo
/// action is accepted by the live action seam.
///
/// `accepted` is the **codegen convention name**: `nmp-codegen` emits, for
/// every app module, an `AppUpdate::<Variant>(<crate>::accepted())` arm on a
/// successful `dispatch_action` — so every app module crate MUST export this
/// exact symbol. (A rejection surfaces through `KernelUpdate::UriRejected`,
/// which the generator builds itself — no per-module shape needed there.)
pub fn accepted() -> Update {
    Update::ActionAccepted
}

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
    use std::ffi::{CStr, CString};

    use nmp_core::{nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string, nmp_app_new};

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

    #[test]
    fn apply_add_then_toggle_then_clear() {
        let mut store = Vec::new();
        apply_todo_action(
            &mut store,
            Action::Add {
                id: "a".to_string(),
                title: "buy milk".to_string(),
            },
        );
        apply_todo_action(
            &mut store,
            Action::Add {
                id: "b".to_string(),
                title: "walk dog".to_string(),
            },
        );
        assert_eq!(store.len(), 2);

        apply_todo_action(&mut store, Action::Toggle { id: "a".to_string() });
        assert!(store.iter().find(|r| r.id == "a").unwrap().completed);

        apply_todo_action(&mut store, Action::ClearCompleted);
        assert_eq!(store.len(), 1);
        assert_eq!(store[0].id, "b");
    }

    #[test]
    fn project_todo_items_reports_open_count() {
        let items = vec![
            TodoRecord {
                id: "a".to_string(),
                title: "open".to_string(),
                completed: false,
            },
            TodoRecord {
                id: "b".to_string(),
                title: "done".to_string(),
                completed: true,
            },
        ];
        let json = project_todo_items(&items);
        assert_eq!(json.get("open_count").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(
            json.get("items").and_then(|v| v.as_array()).map(Vec::len),
            Some(2)
        );
    }

    /// Drive `nmp_app_dispatch_action` for `namespace`/`action_json` against
    /// `app` and return the parsed JSON result. The returned C string is freed.
    fn dispatch(
        app: *mut NmpApp,
        namespace: &str,
        action_json: &str,
    ) -> serde_json::Value {
        let ns = CString::new(namespace).unwrap();
        let body = CString::new(action_json).unwrap();
        let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr());
        assert!(!ptr.is_null(), "dispatch_action must never return null");
        // SAFETY: `ptr` is a valid C string from `nmp_app_dispatch_action`.
        let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        nmp_app_free_string(ptr);
        serde_json::from_str(&out).unwrap()
    }

    /// THE MIGRATION PROOF: after `register`, a `todo.add`
    /// action dispatched through the generic `nmp_app_dispatch_action` path
    /// drives BOTH the host-registered module (`start()` validation) AND the
    /// host-registered executor (`execute()` mutation) end-to-end — the store
    /// gains the record, and the snapshot projection then carries it.
    ///
    /// `KernelSnapshot::projections` is `pub(super)` and unreachable from this
    /// crate, so the snapshot-tick path is proven by `nmp-core`'s own
    /// `snapshot_registry_tests.rs`; here we assert the projection *logic*
    /// produces the right JSON over the store the executor actually mutated.
    #[test]
    fn dispatch_todo_add_lands_in_snapshot_projection() {
        let app = nmp_app_new();
        // SAFETY: `nmp_app_new` never returns null; the pointer is valid until
        // `nmp_app_free` below, and no aliasing `&NmpApp` is live during this
        // exclusive borrow (host-init registration contract).
        let store = register(unsafe { &mut *app });

        // Empty before any dispatch.
        assert!(project_todo_items(&store.lock().unwrap())
            .get("items")
            .and_then(|v| v.as_array())
            .unwrap()
            .is_empty());

        // Dispatch a `todo.add` through the generic action seam.
        let add = Action::Add {
            id: "t1".to_string(),
            title: "ship the fixture".to_string(),
        };
        let parsed = dispatch(
            app,
            ACTION_NAMESPACE,
            &serde_json::to_string(&add).unwrap(),
        );
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
        assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");

        // The executor mutated the host-owned store; the projection carries it.
        let snapshot = project_todo_items(&store.lock().unwrap());
        let items = snapshot.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1, "todo.add must land in the store");
        assert_eq!(
            items[0].get("title").and_then(|v| v.as_str()),
            Some("ship the fixture")
        );
        assert_eq!(
            snapshot.get("open_count").and_then(|v| v.as_u64()),
            Some(1)
        );

        nmp_app_free(app);
    }

    /// An empty-title `todo.add` is rejected by the host module validator at
    /// the `start()` phase — `dispatch_action` returns `{"error":…}` and the
    /// executor never mutates the store (D6: failures are data, not a panic).
    #[test]
    fn dispatch_rejects_empty_title_todo_add() {
        let app = nmp_app_new();
        // SAFETY: see `dispatch_todo_add_lands_in_snapshot_projection`.
        let store = register(unsafe { &mut *app });

        let add = Action::Add {
            id: "t1".to_string(),
            title: "  ".to_string(),
        };
        let parsed = dispatch(
            app,
            ACTION_NAMESPACE,
            &serde_json::to_string(&add).unwrap(),
        );
        let err = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected error object, got {parsed}"));
        assert!(
            err.contains("todo title is empty"),
            "host validator rejection should reach the caller, got: {err}"
        );
        assert!(
            store.lock().unwrap().is_empty(),
            "a rejected action must not mutate the store"
        );

        nmp_app_free(app);
    }
}
