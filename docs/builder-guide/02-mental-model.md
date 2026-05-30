# 02 — Mental model: kernel + extension seams

*Status: SHIPS · Audience: both · Read after [01](01-what-nmp-is.md).*

If you remember one thing: **NMP is a Nostr-native app kernel with first-class
extension modules — not a framework with closed built-ins.** The kernel knows
*how* to run a reactive Nostr client. It does not know *what* a Profile, an
Episode, a Highlight, or a TODO is. Those nouns live in modules you write.

This section gives you the four-layer stack, the three extension seams, the
no-app-nouns-in-kernel rule, what crosses FFI, and a concrete "where does X
live?" map. It is the map for the whole guide.

## The 4-layer stack

Four layers, strict ownership. Built from the bottom up:

```
┌──────────────────────────────────────────────────────────────────────┐
│ PLATFORM SHELL          ios/Chirp + Android Chirp/gallery shells       │
│  owns: rendering, OS handle execution, generated wrappers              │
│  D5 ► consumes ONE bounded FlatBuffers update frame; no policy nouns   │
└────────────────────────────────▲───────────────────────────────────────┘
                                  │ FlatBuffers payload; UniFFI = lifecycle/bindings
┌────────────────────────────────┴───────────────────────────────────────┐
│ GENERATED FFI CRATE     nmp-codegen output (per-app `nmp-app-<name>`)   │
│  owns: concrete AppAction / AppUpdate / ViewSpec enums + FfiApp wrapper  │
│  D6 ► no Result<T,E> crosses here; envelopes only                      │
└────────────────────────────────▲───────────────────────────────────────┘
                                  │ codegen convention exports + NmpApp seams
        ┌─────────────────────────┼──────────────────────────┐
┌───────┴──────────┐  ┌───────────┴───────────┐  ┌────────────┴─────────┐
│ APP CORE CRATES   │  │ NMP PROTOCOL MODULES   │  │  (more app cores)    │
│ apps/chirp/        │  │ nmp-nip29 (groups)     │  │ fixture-todo-core    │
│  nmp-app-chirp     │  │ nmp-nip42 (auth)       │  │  (non-Nostr proof)   │
│                    │  │ nmp-nip77 (sync)       │  │                      │
│ D0 ► MAY hold app  │  │ nmp-signers (identity) │  │ D0 ► app nouns OK    │
│      nouns         │  │ D0 ► protocol nouns ONLY│  │                     │
└───────┬──────────┘  └───────────┬───────────┘  └────────────┬─────────┘
        └─────────────────────────┼──────────────────────────┘
┌────────────────────────────────┴───────────────────────────────────────┐
│ nmp-core KERNEL    actor · EventStore · planner · subs · publish        │
│                    + 2 extension traits + 3 registration seams          │
│  D0 ► ZERO app nouns. ZERO protocol nouns. Generic infrastructure only. │
│  D4 ► one writer per fact (the actor) — never the platform              │
└──────────────────────────────────────────────────────────────────────────┘
```

Representative shipped crates are labelled in their layer above:
`nmp-core` (kernel), `nmp-nip29` / `nmp-nip42` / `nmp-nip77` / `nmp-signers`
(protocol modules), `apps/chirp/nmp-app-chirp` + `fixture-todo-core` (app
cores). `nmp-codegen` produces the generated FFI crate; Chirp is the active
product shell.

### Doctrine callouts on the diagram

- **D0 (kernel/extension boundary).** The dividing line *is* this section.
  `nmp-core` provides generic infrastructure only — actor runtime, verified
  event store, planner, publish pipeline, signer plumbing, the extension
  seams. It contains **no** `Profile`/`Timeline`/`Episode`/`Highlight`/
  `Project` types. The rule: *if shipping your app requires adding a domain
  noun to `nmp-core`, the boundary is wrong and the kernel changes — never
  the app.*
- **D4 (single writer per fact).** Exactly one component owns each fact. The
  actor inside the kernel is that writer. The platform shell never mutates
  state; it renders snapshots and dispatches actions.
- **D5 (snapshots bounded by what's open).** What crosses up to the shell is
  one bounded update payload scoped to currently-open views — not the whole
  store. The runtime payload format is FlatBuffers; the shell holds no
  source-of-truth state.

## The 3 extension seams

Extension crates plug into a vanilla `NmpApp` through exactly three seams
(`crates/nmp-ffi/src/lib.rs:1087-1599`). A crate uses one, two, or all three;
it never reaches into kernel internals.

### Seam 1 — `register_action<M>()`

```rust
app.register_action::<MyActionModule>();
```

Registers an `ActionModule`: its `start()` validates dispatched actions;
its `execute()` enqueues `ActorCommand`s into the actor. The registered module
receives every `nmp_app_dispatch_action(app, NAMESPACE, json)` call whose
`NAMESPACE` matches `MyActionModule::NAMESPACE`.

### Seam 2 — `register_snapshot_projection(key, closure)`

```rust
app.register_snapshot_projection("nmp.myapp.items", move || {
    project_items(&store.lock().unwrap())
});
```

Registers a named JSON slice pushed under `projections["nmp.myapp.items"]` on
every snapshot tick. The closure runs on the **actor thread**; it must be
cheap and non-blocking (D8). Registered under dotted `nmp.*` namespaces.

### Seam 3 — `register_event_observer(arc)`

```rust
app.register_event_observer(Arc::new(MyObserver { store: Arc::clone(&store) }));
```

Registers a `KernelEventObserver` (`actor/commands/event_observer.rs:189`)
for event-driven view updates. `on_event_inserted` / `on_event_replaced` fire
on the actor thread for every accepted ingest. Use this in in-process
consumers (`nmp-app-chirp`, per-app projection crates) that build typed views
from raw `KernelEvent`s.

### The two kernel-defined extension traits

**`ActionModule`** (`substrate/action.rs:56`) — the write seam.

```rust
pub trait ActionModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Action: Clone + Serialize + DeserializeOwned + Send + 'static;

    // Validate `action` upfront. Default: always accept.
    fn start(ctx: &mut ActionContext, action: Self::Action)
        -> Result<(), ActionRejection> { Ok(()) }

    // Optional: suggest a stable correlation_id (e.g. the event id).
    fn preferred_action_id(_action: &Self::Action) -> Option<ActionId> { None }

    // True when the terminal outcome arrives async through
    // projections["action_stages"] rather than the dispatch return value.
    fn is_async_completing() -> bool { false }

    // Enqueue the ActorCommand(s) that carry out the validated action.
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String>;
}
```

**`CapabilityModule`** (`substrate/capability.rs:11`) — the native bridge
shape. Defines typed request/result envelopes; native code implements the
callback and reports raw facts; the kernel decides policy (D7).

```rust
pub trait CapabilityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Request: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Result:  Clone + Serialize + DeserializeOwned + Send + 'static;
    fn callback_interface_name() -> &'static str;
}
```

> **v2 traits that were removed.** An earlier design proposed `ViewModule`,
> `IdentityModule`, and `DomainModule` traits, plus a `ModuleRegistry` that
> collected them. No kernel runtime ever drove them. `substrate/mod.rs`
> documents this explicitly: *"documentation theater that misled readers about
> how extension actually works today."* They are absent from master. The
> correct patterns are the three seams above. See
> [27 — discrepancies](27-discrepancies.md) rows 11–15.

### The codegen convention

`nmp-codegen` generates a per-app FFI crate from each app module. Every app
module crate **must** export these names — codegen reads them by convention
(`crates/nmp-codegen/src/generate.rs`):

| Export | Type | Purpose |
|---|---|---|
| `ACTION_NAMESPACE` | `&'static str` | must equal `MyActionModule::NAMESPACE` |
| `Store` | type alias | app-owned state (`Arc<Mutex<T>>`) |
| `register(app: &mut NmpApp) -> Store` | fn | wires seams, returns store |
| `accepted() -> Update` | fn | success variant for dispatch result |
| `ViewSpec` | enum | host-driven view specs (empty if none) |
| `Update` | enum | update variants (at minimum `ActionAccepted`) |

`register()` is the composition root. From the canonical reference
`apps/fixture/fixture-todo-core/src/lib.rs:122-146`:

```rust
pub fn register(app: &mut NmpApp) -> TodoStore {
    let store = TODO_STORE.get_or_init(|| Arc::new(Mutex::new(Vec::new()))).clone();
    // Seam 1: wire the write path.
    app.register_action::<TodoActionModule>();
    // Seam 2: wire the read path.
    let s = Arc::clone(&store);
    app.register_snapshot_projection(TODO_SNAPSHOT_KEY, move || {
        match s.lock() {
            Ok(g) => project_todo_items(&g),
            Err(_) => serde_json::Value::Null,   // D6: no panic on poison
        }
    });
    store
}
```

## The no-app-nouns-in-kernel rule

This is D0 restated operationally. Before adding a type to `nmp-core`, ask:
*is this generic Nostr-client infrastructure, or is it a noun some specific
app cares about?* `VerifiedEvent`, `CompiledPlan`, `InsertOutcome` are
infrastructure. `Episode`, `Highlight`, `Project`, `Group` are nouns —
protocol nouns go in `nmp-nip*` crates, app nouns in app-core crates. The live
proof that the boundary holds: `fixture-todo-core` exercises all three seams
with zero Nostr concepts, and `nmp-nip29` adds actions + projections for group
machinery while `nmp-core` gains exactly *one* generic seam (the relay-pin
routing lane) and zero group nouns.

## What crosses FFI (and what does not)

| Crosses FFI | Stays in Rust |
|---|---|
| One FlatBuffers update frame per emit (D5) | The EventStore + every `VerifiedEvent` |
| Dispatched `AppAction` variants | Action ledger, ActorCommand queue |
| `CapabilityRequest` / `CapabilityEnvelope` | Planner, subscription pool, signer keys |
| `rev: u64` monotonic guard | All policy / retry / routing decisions |
| `projections[key]` JSON slices | Kernel-internal view state |

No `Result<T,E>` crosses the boundary (D6) — failures arrive as data inside
the snapshot or as capability envelopes. The hot update transport is a single
canonical FlatBuffers schema for `FullState`, `ViewBatch`, and side-effect
frames. Historical raw C JSON-over-string remains live while the FlatBuffers
migration is incomplete (see [15](15-codegen-and-ffi.md) and
[27](27-discrepancies.md) row 3).

## "Where does X live?" — concrete map

| Noun | Lives in | Why |
|---|---|---|
| `VerifiedEvent`, `CompiledPlan` | `nmp-core` | generic Nostr infra |
| `Signer`, keyring access | `nmp-signers` | identity is a protocol module (D0) |
| NIP-29 `GroupId`, group actions | `nmp-nip29` | protocol noun |
| NIP-77 sync reconciler | `nmp-nip77` | protocol noun |
| `TodoRecord`, todo store | `fixture-todo-core` | app noun (non-Nostr proof) |
| App-owned store (`Arc<Mutex<T>>`) | app-core crate | D4: app owns its state |
| SwiftUI list cell, OS audio handle | `ios/Chirp` / shell | rendering / OS execution |

The single test of correctness: a future app module can be added with **zero
changes to `nmp-core`**.

## Anti-patterns

1. **Putting `Highlight` / `Episode` / `Project` in `nmp-core`.** This is the
   exact abstraction error ADR-0009 exists to forbid — it turns the kernel
   into a junk drawer of every consumer's domain concepts. App nouns go in
   app-core crates; protocol nouns in `nmp-nip*` crates.
2. **Reaching for the removed v2 traits.** `ViewModule`, `DomainModule`,
   `IdentityModule`, and `ModuleRegistry` are not on master. Use
   `register_snapshot_projection` for the read path and `register_action`
   for the write path — see [05a](05a-substrate-traits.md).
3. **Bypassing `register_snapshot_projection` to render raw events in
   SwiftUI.** Decoding `kind:1` JSON in Swift re-implements the kernel's
   reactive contract in the shell, duplicates state ownership (D4 violation),
   and breaks D5 bounding. Every read goes through a registered projection or
   a `KernelEventObserver`-driven view.
4. **Adding a 4th registration seam without an ADR.** The three seams are the
   extension contract. A new seam is a kernel change that requires its own ADR.

Paste the **"Where does X live?" map** next to any PR that adds a new type and
answer the "why" column before merging.

See also: [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md) ·
[05a — Kernel substrate — the 2 traits + 3 seams](05a-substrate-traits.md) ·
[15 — Codegen — `nmp gen modules` + per-app FFI crate](15-codegen-and-ffi.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
