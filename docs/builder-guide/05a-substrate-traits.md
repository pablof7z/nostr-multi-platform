# 05a — Kernel substrate: actions, capabilities, and typed reads

*Status: SHIPS · Audience: both · Read after [02](02-mental-model.md).*

[02](02-mental-model.md) gave you the overview. This pair of sections is the
working reference. **05a** = each trait's real signature, associated types,
lifecycle, and a "which seam?" decision tree. **05b** = the annotated
`microblog-core` walkthrough and `nmp-nip29` sidebar.

These are the exact traits and seams in `crates/nmp-core/src/substrate/` and
`crates/nmp-core/src/substrate/`. The kernel runtime is generic over the action
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

    // (The registry always mints the correlation_id — the operation's identity.
    // There is no `preferred_action_id` hook: an action must NEVER substitute
    // output data such as a signed event's id for its identity. See #1748.)

    // True when the action's terminal outcome arrives asynchronously through
    // projections["action_stages"] (signing, relay ack, etc.) rather than as
    // the dispatch return value. Default: false (synchronous settlement).
    fn is_async_completing() -> bool { false }

    // Enqueue the ActorCommand(s) that carry out the validated action.
    // Called after start() returns Ok. `send` is the bridge to the actor's
    // mpsc channel — fire-and-forget, never blocks.
    fn execute(
        &self,
        ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(crate::ActorCommand),
    ) -> Result<(), String>;
}
```

- **Associated types:** `Action` is the input decoded from the typed action
  payload carried by `DispatchEnvelope`.
- **Lifecycle:** `start` validates synchronously with a mutable
  `ActionContext` → if `Ok`, the registry mints the `correlation_id` (the
  operation's sole identity — never an event id) and calls `execute` with the
  same execution context → `execute` calls `send(cmd)` to enqueue
  `ActorCommand`(s)
  → actor processes them → outcome surfaces in the snapshot (D6: never as an
  exception across FFI).
- **State:** modules are registered as values, so a stateful module carries
  its dependencies in `self`. `ActionContext` carries execution-scoped runtime
  capabilities such as bounded, cache-only local-store reads
  (`local_event_by_id`, `query_local_events`). It never opens relays or waits
  for acquisition.
- **Use it when** any user or app intent dispatches an action. Every published
  event, every follow/unfollow, every settings write goes through `execute`.

### Registration

```rust
// In your module's register() fn:
app.register_action(MyActionModule);
```

One call. The registered module handles every `DispatchEnvelope` whose action
namespace matches `MyActionModule::NAMESPACE`.

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
  Start/stop must be idempotent and safe N times. Native public bindings use
  UniFFI capability objects; browser bindings use wasm-bindgen adapters.
- **Use it when** you need an OS handle (keychain, push, audio, network
  monitor). Native code *reports a fact*; it never decides retry, routing,
  or any policy (D7). Results are envelopes, not `Result`-typed errors.

## Concept-owned active reads — the read surface

Production app code opens a **concept-owned active read**: the thing that wants
to show a fact asks the owner of that fact for it. The concept owner exposes a
concrete `open_<concept>(target)` helper that returns a close handle and owns
demand, replay-before-live, status, output, dynamic source wakeups, and
teardown. It may register typed output and may use observed delivery and internal
refcounting privately, but product screens do not wire raw interests, observers,
replay sidecars, or source reducers — and there is **no** generic `Claim` /
`Release` verb or `open_session(namespace, bytes)` doorway
([#2508](https://github.com/pablof7z/nostr-multi-platform/issues/2508)).

```rust
// A concept owner registers its helper; the shell calls open_<concept> to mount.
register_topic_articles_concept(app);
let handle = open_topic_articles(&app, "bitcoin", "discover-view"); // close via handle
```

- **Contract:** the concept owner declares the read shape and owns all
  acquisition and close behavior. The shell opens the concept by name and drops
  the returned handle to close it, rendering the pushed typed output.
- **Use it when** a product screen needs Nostr data. The app asks the concept's
  owner (`nmp-nip25` for reactions, the replies owner for replies, an app crate
  for app-specific facts); Rust owns the route, replay, filtering, and state.
- **Never:** expose a generic claim/release/session doorway, or hand-author raw
  interest, observed-delivery, or source-reduction plumbing from product or
  native shell code. "Session" is internal runtime bookkeeping, not a public noun.

## register_typed_snapshot_projection — internal read output transport

Registers a typed sidecar pushed in every snapshot tick under
`typed_projections[key]`.

```rust
// Framework/protocol crate (concept owner) — a declared `nmp.*` key citing
// the ownership claim the crate registers for it.
app.register_typed_snapshot_projection(
    nmp_ownership::DeclaredProjectionKey::framework(
        "nmp.mymodule.state",
        "projection.nmp.mymodule.state",
    ),
    move || store.lock().ok().map(|g| encode_mymodule_snapshot(&g)),
);
```

- **Contract:** the closure runs on the **actor thread** inside every snapshot
  tick. It MUST be cheap and non-blocking (D8: no I/O, no mutex waits that
  could block relay ingest). A panic inside is isolated (`catch_unwind` per
  closure, D6).
- **Key naming:** `register_typed_snapshot_projection` takes anything that
  implements `Into<ProjectionRegistrationKey>` (`nmp-ownership`), not a raw
  string (PR #2610). Framework/protocol crates declare a `nmp.<module>.*` key
  through `DeclaredProjectionKey::framework(key, owner_claim)`, citing the
  ownership claim in their `nmp-ownership` descriptor — `nmp crate-ownership
  audit --deny` verifies the pair. App/product code uses
  `ProjectionKey::app_owned(...)` / `DynamicProjectionKey::app_owned(...)`
  with a non-`nmp.*` namespace instead; `nmp.*` is reserved for declared
  framework surfaces. Kernel-reserved keys (`publish_queue`, `accounts`,
  `profile`, views cluster) always win on collision.
- **Use it when** you are implementing a concept-owned active read or protocol
  executor and need its state visible in the host's `apply()` callback alongside
  the built-in named fields. Product screens should open the helper, not wire
  this transport seam directly.

## Observed delivery — internal concept-executor machinery

Some concept/protocol executors maintain an in-process read model from Nostr
events. They do that with declared observed delivery after the read model has
declared the events it needs. The kernel registers the sink muted, opens the
declared interest, replays matching cached/store-backed rows, then activates
future delivery scoped to the declaration.

```rust
pub trait ObservedProjectionSink: Send + Sync {
    // Fires only for events matching the projection's declared shapes.
    fn on_kernel_event(&self, event: &KernelEvent);
}

let observer_id = app.open_observed_projection(ObservedProjection::from_kinds(
    Arc::new(MyObserver { store: arc_store.clone() }),
    "myapp.items", // refcount consumer id — app-owned, not a projection key
    0,
    [KIND_NOTE],
    128,
));
```

- **Lifecycle:** the sink body runs synchronously on the **actor thread**. Must
  be cheap; no blocking I/O. Duplicates, supersessions, and rejections do NOT
  fire the observer.
- **Use it when** you are implementing a concept-owned active read or reusable
  protocol executor. The declaration must name the scope by kind, author, id,
  tag, relay pin, search shape, source reducer, or bounded dependency before
  events are delivered.
- **Never:** production app/product code must not register a blanket all-event
  observer or expose observed delivery as its app API. A remaining event slot is
  kernel-internal plumbing only.

## Decision tree: "I want X — which seam?"

```
I want to ...
│
├─ change state, publish, or mutate anything    → ActionModule + register_action
│     └─ result must survive restart / relay ack   use is_async_completing = true
│
├─ read Nostr data for a product screen       → concept-owned active read
│     └─ open_<concept>(target) → handle; concept owner owns demand+replay+output+teardown
│
├─ expose concept-owned typed output          → register_typed_snapshot_projection
│     └─ internal transport; cheap + non-blocking closure
│
├─ maintain concept-owned event projection    → declared observed delivery
│     └─ internal concept/protocol executor machinery
│
├─ report OS-native facts to the kernel        → CapabilityModule
│     (keychain, push, audio, network)             (UniFFI capability object)
│
└─ none of these — pure app-local state        → Arc<Mutex<T>> in register()
      (in-memory store, no relay traffic)          no kernel seam needed
```

A real app typically combines several: `microblog-core` uses
`register_action` + a concept-owned active read + typed output transport.
Walkthroughs are in
[05b](05b-substrate-traits.md) and [19a](19a-walkthrough-microblog.md).

## Removed v2 traits (reference)

An earlier proposed v2 extension architecture included `ViewModule`,
`DomainModule`, and `IdentityModule` traits, plus a `ModuleRegistry` to
collect them. These were **removed before shipping** — no kernel runtime ever
drove them. `crates/nmp-core/src/substrate/mod.rs` documents this history.

If you encounter references to these types in older docs, ADRs, or codegen
output, treat them as stale. The correct replacements:

| Removed concept | Replacement |
|---|---|
| `ViewModule` (typed reactive projection) | concept-owned active read + typed output |
| `DomainModule` (kernel-owned domain store) | app-owned `Arc<Mutex<T>>` + typed output |
| `IdentityModule` (signer scope) | `nmp-signers` crate + keyring capability |
| `ModuleRegistry` (composition root) | an app-core `register()` fn that installs explicit substrate/protocol/app features |
| `ActionPlan` / `ActionTransition` / `reduce()` | `execute()` dispatching `ActorCommand` |

## Deliverables (this half)

- **Per-seam shape block** (above) — copy the skeleton, fill the types,
  delete the comments.
- **"Which seam?" decision tree** (above) — answer it before opening any
  PR that adds a module.

See also: [02 — Mental model](02-mental-model.md) ·
[05b — Substrate traits: microblog walkthrough + nip29 + composition](05b-substrate-traits.md) ·
[06 — Reactivity contract (D8)](06-reactivity-contract.md) ·
[16 — Capabilities (D7)](16-capabilities.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
