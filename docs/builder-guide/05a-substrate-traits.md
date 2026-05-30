# 05a — Kernel substrate: the 2 traits + 3 seams

*Status: SHIPS · Audience: both · Read after [02](02-mental-model.md).*

[02](02-mental-model.md) gave you the overview. This pair of sections is the
working reference. **05a** = each trait's real signature, associated types,
lifecycle, and a "which seam?" decision tree. **05b** = the annotated
`fixture-todo-core` walkthrough and `nmp-nip29` sidebar.

These are the exact traits and seams in `crates/nmp-core/src/substrate/` and
`crates/nmp-ffi/src/lib.rs`. The kernel runtime is generic over the action
trait: it never names your `Action` type, only that your module conforms.

## ActionModule — the write seam

`crates/nmp-core/src/substrate/action.rs:56-121`. For anything that mutates
state, dispatches to relays, or coordinates a multi-step operation.

```rust
pub trait ActionModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;   // dispatch key, e.g. "fixture.todo.action"

    type Action: Clone + Serialize + DeserializeOwned + Send + 'static;

    // Validate `action` upfront. `Ok(())` accepts; `Err` rejects with a
    // message surfaced through the dispatch return JSON. Default: always accept.
    fn start(ctx: &mut ActionContext, action: Self::Action)
        -> Result<(), ActionRejection> { Ok(()) }

    // Optional: suggest the correlation_id the registry should assign.
    // Useful when the action's natural identity is already stable (e.g. an
    // event id for a publish action). Default: auto-generated 32-hex string.
    fn preferred_action_id(_action: &Self::Action) -> Option<ActionId> { None }

    // True when the action's terminal outcome arrives asynchronously through
    // projections["action_stages"] (signing, relay ack, etc.) rather than as
    // the dispatch return value. Default: false (synchronous settlement).
    fn is_async_completing() -> bool { false }

    // Enqueue the ActorCommand(s) that carry out the validated action.
    // Called after start() returns Ok. `send` is the bridge to the actor's
    // mpsc channel — fire-and-forget, never blocks.
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(crate::ActorCommand),
    ) -> Result<(), String>;
}
```

- **Associated types:** `Action` is the input — whatever the host serializes
  and passes to `nmp_app_dispatch_action`.
- **Lifecycle:** `start` validates synchronously → if `Ok`, the registry
  mints a `correlation_id` (or uses the one from `preferred_action_id`) and
  calls `execute` → `execute` calls `send(cmd)` to enqueue `ActorCommand`(s)
  → actor processes them → outcome surfaces in the snapshot (D6: never as an
  exception across FFI).
- **State:** none on the trait. App state lives in an `Arc<Mutex<T>>` owned
  by the app module, reached from `execute` via a `static OnceLock` or
  equivalent process-wide slot. See `fixture-todo-core`'s `TODO_STORE` pattern
  in [05b](05b-substrate-traits.md).
- **Use it when** any user or app intent dispatches an action. Every published
  event, every follow/unfollow, every settings write goes through `execute`.

### Registration

```rust
// In your module's register() fn:
app.register_action::<MyActionModule>();
// crates/nmp-ffi/src/lib.rs:1087
```

One call. The registered module handles every
`nmp_app_dispatch_action(app, MyActionModule::NAMESPACE, json)` call.

## CapabilityModule — the native bridge shape

`crates/nmp-core/src/substrate/capability.rs:11-24`. Defines the typed
request/result envelope a native capability bridge uses. Native code *reports
raw facts*; the kernel decides policy (D7).

```rust
pub trait CapabilityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;   // e.g. "fixture.todo.capability"
    type Request: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Result:  Clone + Serialize + DeserializeOwned + Send + 'static;
    fn callback_interface_name() -> &'static str;   // native bridge name
}
// Wire types:
// CapabilityRequest  { namespace, correlation_id, payload_json }
// CapabilityEnvelope { namespace, correlation_id, result_json }
```

- **Lifecycle:** kernel emits a `CapabilityRequest` → native side executes
  the OS handle → returns a `CapabilityEnvelope` keyed by `correlation_id`.
  Start/stop must be idempotent and safe N times. The native side is wired
  via C-ABI callbacks, not via a Rust registration call.
- **Use it when** you need an OS handle (keychain, push, audio, network
  monitor). Native code *reports a fact*; it never decides retry, routing,
  or any policy (D7). Results are envelopes, not `Result`-typed errors.

## register_snapshot_projection — the read output seam

`crates/nmp-ffi/src/lib.rs:1109`. Registers a named JSON slice pushed in
every snapshot tick under `KernelSnapshot.projections[key]`.

```rust
app.register_snapshot_projection("nmp.myapp.key", move || {
    serde_json::json!({ "count": store.lock().unwrap().len() })
});
```

- **Contract:** the closure runs on the **actor thread** inside every snapshot
  tick. It MUST be cheap and non-blocking (D8: no I/O, no mutex waits that
  could block relay ingest). A panic inside is isolated (`catch_unwind` per
  closure, D6).
- **Key naming:** use `nmp.<module>.*` namespaces. Kernel-reserved keys
  (`publish_queue`, `accounts`, `profile`, views cluster) always win on
  collision.
- **Use it when** you want module state visible in the host's `apply()`
  callback alongside the built-in named fields.

## register_event_observer — the event-driven view seam

`crates/nmp-ffi/src/lib.rs:1590`. Registers an in-process `KernelEventObserver`
for event-driven view updates.

```rust
pub trait KernelEventObserver: Send + Sync {
    // Fires for every event accepted by EventStore::insert (Inserted | Replaced).
    // Duplicates, supersessions, and rejections do NOT fire this method.
    fn on_kernel_event(&self, event: &KernelEvent);
}

app.register_event_observer(Arc::new(MyObserver { store: arc_store.clone() }));
// returns KernelEventObserverId for later unregister_event_observer()
```

- **Lifecycle:** fires synchronously on the **actor thread** for every
  `Inserted | Replaced` ingest outcome. Must be cheap; no blocking I/O.
  Duplicates, supersessions, and rejections do NOT fire the observer.
  This is the mechanism per-app crates use to build typed timeline views
  (`nmp-app-chirp` registers an observer that drives the modular timeline
  projection).
- **Use it when** you need to maintain an in-process projection that updates
  on every event arrival — e.g. a timeline sorted by kind:1 events.

## Decision tree: "I want X — which seam?"

```
I want to ...
│
├─ change state, publish, or mutate anything    → ActionModule + register_action
│     └─ result must survive restart / relay ack   use is_async_completing = true
│
├─ expose a named JSON slice to the host shell  → register_snapshot_projection
│     └─ cheap + non-blocking closure
│
├─ maintain an in-process typed projection      → KernelEventObserver
│     (timeline, group chat list, …)               + register_event_observer
│
├─ report OS-native facts to the kernel        → CapabilityModule
│     (keychain, push, audio, network)             (native C-ABI callback)
│
└─ none of these — pure app-local state        → Arc<Mutex<T>> in register()
      (in-memory store, no relay traffic)          no kernel seam needed
```

A real app typically combines several: `fixture-todo-core` uses
`register_action` + `register_snapshot_projection`; `nmp-app-chirp` adds
`register_event_observer` for the live timeline feed. Walkthroughs are in
[05b](05b-substrate-traits.md).

## Removed v2 traits (reference)

An earlier proposed v2 extension architecture included `ViewModule`,
`DomainModule`, and `IdentityModule` traits, plus a `ModuleRegistry` to
collect them. These were **removed before shipping** — no kernel runtime ever
drove them. `crates/nmp-core/src/substrate/mod.rs` documents this history.

If you encounter references to these types in older docs, ADRs, or codegen
output, treat them as stale. The correct replacements:

| Removed concept | Replacement |
|---|---|
| `ViewModule` (typed reactive projection) | `register_event_observer` + `register_snapshot_projection` |
| `DomainModule` (kernel-owned domain store) | app-owned `Arc<Mutex<T>>` + `register_snapshot_projection` |
| `IdentityModule` (signer scope) | `nmp-signers` crate + keyring capability |
| `ModuleRegistry` (composition root) | `register()` fn in each module crate, called in `FfiApp::new` |
| `ActionPlan` / `ActionTransition` / `reduce()` | `execute()` dispatching `ActorCommand` |

See [27 — discrepancies](27-discrepancies.md) rows 11–15 for the triage
record.

## Deliverables (this half)

- **Per-seam shape block** (above) — copy the skeleton, fill the types,
  delete the comments.
- **"Which seam?" decision tree** (above) — answer it before opening any
  PR that adds a module.

See also: [02 — Mental model](02-mental-model.md) ·
[05b — Substrate traits: fixture walkthrough + nip29 + composition](05b-substrate-traits.md) ·
[06 — Reactivity contract (D8)](06-reactivity-contract.md) ·
[16 — Capabilities (D7)](16-capabilities.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
