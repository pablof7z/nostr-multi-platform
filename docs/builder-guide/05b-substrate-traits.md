# 05b — Kernel substrate: fixture walkthrough + nip29 + composition

*Status: SHIPS · Audience: both · Read after [05a](05a-substrate-traits.md).*

[05a](05a-substrate-traits.md) gave you the seam signatures and the "which
seam?" tree. This half is the proof the boundary works: an annotated non-Nostr
fixture that uses all three seams, a sidebar showing how a real Nostr protocol
crate uses them, and how modules compose at `FfiApp::new`.

## Annotated walkthrough: `fixture-todo-core`

`apps/fixture/fixture-todo-core/src/lib.rs` is ADR-0009 acceptance criterion
1 made real: a module exercising the extension seams **with zero Nostr
concepts**. It is the canonical template — read it before writing any module.

### The record type

```rust
// lib.rs:161-165 — plain app record; not a Nostr event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TodoRecord {
    pub id: String,
    pub title: String,
    pub completed: bool,
}
```

The kernel never sees `TodoRecord`. It is an app noun that lives entirely in
this crate (D0). The kernel sees only the JSON it receives from
`nmp_app_dispatch_action` and produces as a `projections[key]` slice.

### The action enum and ActionModule

```rust
// lib.rs:167-172
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Action {
    Add { id: String, title: String },
    Toggle { id: String },
    ClearCompleted,
}

// lib.rs:176-215
impl ActionModule for TodoActionModule {
    const NAMESPACE: &'static str = "fixture.todo.action";
    type Action = Action;

    fn start(_ctx: &mut ActionContext, action: Self::Action)
        -> Result<(), ActionRejection> {
        // Reject bad input synchronously — before any state is touched.
        if matches!(&action, Action::Add { title, .. } if title.trim().is_empty()) {
            return Err(ActionRejection::Invalid("todo title is empty".to_string()));
        }
        Ok(())
    }

    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        // Reach the app-owned store via the process-wide OnceLock.
        let store = TODO_STORE.get()
            .ok_or_else(|| "register() not called before execute()".to_string())?;
        apply_todo_action(&mut store.lock().map_err(|_| "mutex poisoned")?, action);
        Ok(())
    }
}
```

Key teaching points:
- `start` rejects bad input *synchronously*. The executor never runs.
- `execute` dispatches **no** `ActorCommand` here — the todo flow is
  app-local. `_send` is unused. A Nostr-publishing action would call
  `send(ActorCommand::PublishUnsignedEvent { .. })` instead.
- App state (`TODO_STORE`) is a `static OnceLock<Arc<Mutex<Vec<TodoRecord>>>>`.
  The `execute` body is a static method (no `&self`), so it reads the store
  from the process-wide slot that `register()` initializes.

### The snapshot projection

```rust
// lib.rs:74-80 — pure fn; no FFI, no actor.
pub fn project_todo_items(items: &[TodoRecord]) -> serde_json::Value {
    let open_count = items.iter().filter(|r| !r.completed).count();
    serde_json::json!({ "items": items, "open_count": open_count })
}
```

The closure registered in `register()` delegates here:

```rust
// lib.rs:135-143
let projector_store = Arc::clone(&store);
app.register_snapshot_projection(TODO_SNAPSHOT_KEY, move || {
    match projector_store.lock() {
        Ok(guard) => project_todo_items(&guard),
        Err(_)    => serde_json::Value::Null,   // D6: no panic on poison
    }
});
```

This is D8 + D6 in one line: the closure is cheap (one lock + JSON), and
panic-safe (returns `null` on mutex poison rather than aborting the snapshot
tick).

### The CapabilityModule

```rust
// lib.rs:218-238
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CapabilityCall  { CountOpenTodos }
pub struct CapabilityResult { pub count: usize }

impl CapabilityModule for TodoCapabilityModule {
    const NAMESPACE: &'static str = "fixture.todo.capability";
    type Request = CapabilityCall;
    type Result  = CapabilityResult;
    fn callback_interface_name() -> &'static str { "FixtureTodoCapability" }
}
```

This defines the typed envelope shape a native bridge would implement. Native
*reports* the count (a fact); the kernel decides what to do with it (policy).
The trait is not registered with `register_action` — it is wired by the native
side through C-ABI callbacks.

### The codegen convention exports

```rust
// lib.rs:7-33 — the codegen contract
pub const ACTION_NAMESPACE: &str = TodoActionModule::NAMESPACE;
pub const TODO_SNAPSHOT_KEY: &str = "fixture.todo.items";
pub type TodoStore = Arc<Mutex<Vec<TodoRecord>>>;
pub type Store = TodoStore;   // codegen reads this exact alias name

pub fn register(app: &mut NmpApp) -> TodoStore { /* seam wiring */ }
pub fn accepted() -> Update { Update::ActionAccepted }

pub enum ViewSpec {}          // empty — no host-driven view specs
pub enum Update { ActionAccepted }
```

`nmp-codegen` generates a `FfiApp` that calls `fixture_todo_core::register(&mut *app)`
in `FfiApp::new`, stores the returned `Store`, and routes
`AppAction::FixtureTodoCore(action)` through `dispatch_app_action(ACTION_NAMESPACE, …)`.
**Never hand-edit the generated crate** — `generate_modules` wipes the `src/`
directory on every run.

### What `fixture-todo-core` proves

1. A complete app module with writes (ActionModule), reads
   (register_snapshot_projection), and a capability shape
   (CapabilityModule) — **without touching `nmp-core`**.
2. App state is app-owned (`Arc<Mutex<Vec<TodoRecord>>>`). The kernel never
   stores, migrates, or indexes `TodoRecord`. The module owns its data.
3. Validation is synchronous (`start`). Execution is synchronous for
   local-only work; async-completing actions use `is_async_completing()`.

## Sidebar: how `nmp-nip29` uses the seams

`crates/nmp-nip29/src/lib.rs` is the Nostr-shaped counterpart — the proof the
same seams scale to a real protocol with **zero new nouns in `nmp-core`**.

```
nmp-nip29/src/
├── action/          15 ActionModule impls (CreateGroup, JoinGroup, PostChat, …)
├── cache/           protocol-local caches (TOFU signer, recent events)
├── projection/      read model: NIP-29 group-chat aggregate
├── group_id.rs      GroupId { host_relay_url, local_id } — protocol noun
├── interest.rs      helpers building LogicalInterests with relay_pin
├── kinds.rs         NIP-29 kind constants + dispatch helper
├── register.rs      register_actions(app: &mut NmpApp) + snapshot projector
└── lib.rs           D0 boundary statement + register() public surface
```

Registration (`crates/nmp-nip29/src/register.rs:105`):

```rust
pub fn register_actions(app: &mut NmpApp) {
    app.register_action::<PostChatMessageAction>();
    app.register_action::<ReactInGroupAction>();
    app.register_action::<CreatePublicGroupAction>();
    app.register_action::<DiscoverGroupsAction>();
    app.register_action::<JoinGroupAction>();
    // … 10 more
}
```

And the snapshot projector:

```rust
// register.rs:66 — register the group-chat aggregate read model
app.register_snapshot_projection("nmp.nip29.group_chat", move || {
    projection.snapshot_json()  // non-blocking read-model snapshot
});
```

The crate-boundary statement at `lib.rs:10–19` is the doctrine in code:
*`nmp-nip29` does NOT import any other `nmp-nip*` crate; cross-protocol
composition happens at the app layer; the only generic surface added to
`nmp-core` is the third routing lane (`relay_pin` + lattice Rule 9).*

> **What happened to "13 Domain + 7 View modules"?** Earlier docs cited
> `domain/mod.rs` and `view/mod.rs` directories with `register_all()` fns.
> Those reflected the removed v2 DomainModule/ViewModule architecture. The
> current crate has `action/` for ActionModules and `projection/` for the
> read model. The protocol noun count (~35 named types) is similar — the
> *composition mechanism* changed, not the scope.

## Module composition in `FfiApp::new`

Every module's `register()` fn is called once at host init. Generated
`FfiApp::new` (`nmp-app-fixture/src/ffi.rs:54-66`) chains them:

```rust
pub fn new() -> Self {
    let app = nmp_app_new();
    // Each module wires its own seams; store handles returned for the fields below.
    let fixture_todo_core_store = fixture_todo_core::register(unsafe { &mut *app });
    Self { app, kernel: KernelReducer::new(), rev: 0, fixture_todo_core_store }
}
```

For a multi-module app with protocol + app crates:

```rust
// generated FfiApp::new (conceptual; actual output depends on nmp.toml)
pub fn new() -> Self {
    let app = nmp_app_new();
    let raw = unsafe { &mut *app };
    nmp_nip29::register_actions(raw);          // protocol module
    let store = my_app_core::register(raw);    // app module
    Self { app, kernel: KernelReducer::new(), rev: 0, my_app_core_store: store }
}
```

Registration order is deterministic — `ordered_modules()` in
`nmp-codegen/src/manifest.rs:67` chains `protocol` entries before `app`
entries in manifest order. Two modules registering the same `ACTION_NAMESPACE`
would collide at dispatch time; NAMESPACE values must be globally unique
across all registered modules.

## Anti-patterns

1. **App state inside the kernel.** The todo store is an `Arc<Mutex<Vec<…>>>`
   owned by `fixture-todo-core`, not by `nmp-core`. Pushing app records into
   the kernel event store or a kernel-owned map is a D0 violation.
2. **Business policy in a `CapabilityModule` (D7 violation).** The fixture's
   `CountOpenTodos` returns a count fact. It must not decide retry, routing,
   or "should we publish." Policy lives in the `ActionModule::execute` body.
3. **Blocking inside `register_snapshot_projection`.** The closure runs on the
   actor thread inside every snapshot tick. Any blocking I/O or long-held lock
   stalls all relay ingest behind it (D8 violation). Delegate to a precomputed
   value; the snapshot projector should read, never compute.
4. **Registering the same NAMESPACE twice.** `register_action` accepts the
   second registration silently (idempotent by `(namespace)` key), but two
   modules sharing a NAMESPACE will race for dispatch. Pick unique dotted
   namespaces per module.
5. **Reaching for the removed v2 traits.** `DomainModule`, `ViewModule`,
   `IdentityModule`, and `ModuleRegistry` are not on master. See
   [05a](05a-substrate-traits.md) §Removed v2 traits.

## Deliverables (this half)

- **Annotated `fixture-todo-core` walkthrough** (above) — the copyable
  two-seam template: ActionModule + snapshot projection.
- **`nmp-nip29` sidebar** (above) — how the same seams scale to a real
  protocol with zero kernel nouns; plus the composition pattern.

See also: [02 — Mental model](02-mental-model.md) ·
[05a — Substrate traits: signatures + decision tree](05a-substrate-traits.md) ·
[06 — Reactivity contract (D8)](06-reactivity-contract.md) ·
[16 — Capabilities (D7)](16-capabilities.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
