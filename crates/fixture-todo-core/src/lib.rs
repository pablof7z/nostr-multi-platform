use std::sync::{Arc, Mutex, OnceLock};

use nmp_core::substrate::*;
use nmp_ffi::NmpApp;
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

/// Process-wide handle to the todo store the typed [`TodoActionModule::execute`]
/// reads. ADR-0027's `ActionModule::execute(action, correlation_id, send)` is a
/// static method — no `&self`, no instance state — so the host-owned `TodoStore`
/// has to live in a place the static body can reach. A `OnceLock<TodoStore>` is
/// the minimum-viable shape: `register()` initializes it (returning the same
/// `Arc` to the host), and `execute()` upgrades to a clone of that `Arc` on
/// every dispatch.
///
/// The codegen contract is "one `FfiApp` per process" — `register()` is called
/// once per FFI instance — so the `OnceLock` is the correct cardinality. A
/// re-`register()` call returns the previously-initialized store rather than
/// installing a fresh one; this keeps the snapshot projection coherent across
/// re-init (test-only scenario; the production codegen path runs it once in
/// `FfiApp::new`).
static TODO_STORE: OnceLock<TodoStore> = OnceLock::new();

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
/// * an **action module** via [`NmpApp::register_action::<TodoActionModule>`] —
///   ADR-0027's single-call typed seam. `TodoActionModule::start` rejects an
///   empty title; `TodoActionModule::execute` applies the validated action to
///   the host-owned store reached through the `TODO_STORE` `OnceLock`;
/// * a **snapshot projection** under [`TODO_SNAPSHOT_KEY`] — projects the
///   store into JSON via [`project_todo_items`] on every snapshot tick.
///
/// After this call, `nmp_app_dispatch_action(app, ACTION_NAMESPACE, …)`
/// drives a todo action end-to-end: `start()` validates, `execute()` mutates
/// the store, and the next snapshot carries the projected list.
///
/// MUST be called during host init — before `nmp_app_start` and before any
/// `nmp_app_dispatch_action` — because [`NmpApp::register_action`] takes
/// `&mut NmpApp`.
pub fn register(app: &mut NmpApp) -> TodoStore {
    // First-init wins: a second `register()` call returns the existing store
    // so the typed `execute()` and the snapshot projection share one `Arc`.
    let store: TodoStore = TODO_STORE
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone();

    // ADR-0027 typed seam — one call wires both `start()` validation and
    // `execute()` against `TodoActionModule::NAMESPACE` (== `ACTION_NAMESPACE`).
    app.register_action::<TodoActionModule>();

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Action {
    Add { id: String, title: String },
    Toggle { id: String },
    ClearCompleted,
}

pub struct TodoActionModule;

impl ActionModule for TodoActionModule {
    const NAMESPACE: &'static str = "fixture.todo.action";

    type Action = Action;

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        if matches!(&action, Action::Add { title, .. } if title.trim().is_empty()) {
            return Err(ActionRejection::Invalid("todo title is empty".to_string()));
        }
        Ok(())
    }

    /// Apply the validated action to the host-owned todo store reached through
    /// the [`TODO_STORE`] `OnceLock`.
    ///
    /// `register()` initializes the lock — the typed `execute()` body is only
    /// reachable AFTER `register::<TodoActionModule>()` ran (per the host-init
    /// contract on `register_action`), so `TODO_STORE.get()` is always `Some`
    /// here in practice. A `None` from a misordered host init is surfaced as
    /// `Err` rather than panicking (D6).
    ///
    /// The todo flow is local-only: no [`nmp_core::ActorCommand`] is needed,
    /// so the `send` bridge is deliberately unused.
    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        let store = TODO_STORE
            .get()
            .ok_or_else(|| "fixture-todo-core: register() not called before execute()".to_string())?;
        let mut guard = store
            .lock()
            .map_err(|_| "todo store mutex poisoned".to_string())?;
        apply_todo_action(&mut guard, action);
        Ok(())
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

/// The projected view-spec enum for this app module — the **codegen
/// convention name** `<crate>::ViewSpec`. `nmp-codegen` emits a
/// `FixtureTodoCore(fixture_todo_core::ViewSpec)` variant in the generated
/// fixture app's `ViewSpec` enum, so every app module crate MUST export this
/// exact symbol. The fixture's todo flow exposes no views, so the enum is
/// intentionally empty (uninhabited) — the wrapping variant simply never gets
/// constructed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ViewSpec {}

/// The projected update enum for this app module — the **codegen convention
/// name** `<crate>::Update`. `nmp-codegen` emits a
/// `FixtureTodoCore(fixture_todo_core::Update)` variant in the generated
/// fixture app's `AppUpdate` enum, so every app module crate MUST export this
/// exact symbol. [`accepted`] returns [`Update::ActionAccepted`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Update {
    ActionAccepted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    use nmp_ffi::{nmp_app_dispatch_action, nmp_app_free, nmp_app_free_string, nmp_app_new};

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

    /// Serialize FFI tests that share the process-wide `TODO_STORE`. ADR-0027's
    /// typed `execute()` is static, so `register()` writes into a `OnceLock`
    /// shared across every test in this binary; cargo runs unit tests in
    /// parallel by default, so two FFI tests dispatching against the same
    /// store would race. Each FFI test takes this `Mutex` for its full body —
    /// the lifetime of the `MutexGuard` IS the serialization fence.
    fn ffi_test_guard() -> std::sync::MutexGuard<'static, ()> {
        static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
        // A poisoned mutex still hands out a guard via `into_inner` — a prior
        // panic in another test must not silently disable serialization here.
        GUARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reset the process-wide `TODO_STORE` so the calling FFI test sees an
    /// empty store regardless of which other test ran before it.
    fn clear_store_for_test() {
        if let Some(store) = TODO_STORE.get() {
            if let Ok(mut guard) = store.lock() {
                guard.clear();
            }
        }
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
        let _guard = ffi_test_guard();
        clear_store_for_test();
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
        let _guard = ffi_test_guard();
        clear_store_for_test();
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
