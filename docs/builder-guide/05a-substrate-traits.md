# 05a — Kernel substrate: the 5 trait families (signatures + decision tree)

*Status: SHIPS · Audience: both · Read after [02](02-mental-model.md).*

[02](02-mental-model.md) gave you the one-paragraph version. This pair of
sections is the working reference. **05a** = each family's real signature,
associated types, lifecycle, and a "which trait?" decision tree. **05b** =
the annotated `fixture-todo-core` walkthrough, the `nmp-nip29` sidebar, and
`ModuleRegistry` composition.

These are the exact traits in `crates/nmp-core/src/substrate/`. The kernel
runtime is generic over them: it never names your `Spec` or `Action` type,
only that your module conforms (`kernel-substrate.md` §1).

## DomainModule — durable non-Nostr records

`crates/nmp-core/src/substrate/domain.rs:1-49`. For state that is not a Nostr
event but must survive restart: drafts, settings, transcripts, weight logs.

```rust
pub trait DomainModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;            // unique, e.g. "fixture.todo.domain"
    const SCHEMA_VERSION: u32;

    // Kinds this module decodes at ingest. Default &[] = pure app-local
    // store (no relay traffic). Protocol crates override to claim kinds.
    fn ingest_kinds() -> &'static [u32] { &[] }

    fn migrations() -> Vec<DomainMigration>;   // versioned, applied in order
    fn indexes() -> Vec<DomainIndex>;          // secondary keys the kernel maintains
    fn register(registry: &mut DomainRegistry);// declare record types
}
```

- **Associated state:** none on the trait — records are bytes the kernel
  stores under `NAMESPACE`; `DomainIndex.key_fn` extracts a secondary key
  from a serialized record.
- **Lifecycle:** `register` at startup → `migrations()` run from the stored
  `SCHEMA_VERSION` forward → CRUD through kernel-owned storage.
- **Use it when** you have durable records with no Nostr identity. Empty
  `ingest_kinds()` means app-local writes; override it (e.g. `&[39000]`) to
  own a Nostr kind in a protocol crate. Per D4 each `(kind, discriminator)`
  pair has exactly one owning module.

## ViewModule — typed reactive projections

`crates/nmp-core/src/substrate/view.rs:37-80`. The only sanctioned path for
state to reach the UI.

```rust
pub trait ViewModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Spec:    Clone + Serialize + DeserializeOwned + Send + 'static;
    type Payload: Clone + Serialize + Send + 'static;
    type Delta:   Clone + Serialize + Send + 'static;
    type Key:     Hash + Eq + Clone + Serialize + Send + 'static;
    type State:   Send + 'static;

    fn key(spec: &Self::Spec) -> Self::Key;                 // identity / dedupe
    fn dependencies(spec: &Self::Spec) -> ViewDependencies; // composite keys!
    fn open(ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload);
    fn on_event_inserted(ctx, &mut State, &KernelEvent) -> Option<Self::Delta>;
    fn on_event_removed (ctx, &mut State, &EventId)     -> Option<Self::Delta>;
    fn on_event_replaced(ctx, &mut State, &EventId, &KernelEvent) -> Option<Delta>;
    fn on_projection_changed(ctx, &mut State, &ProjectionChange) -> Option<Delta>;
    fn on_tick(ctx, &mut State) -> Option<Self::Delta> { None } // default: inert
    fn snapshot(ctx: &ViewContext, state: &Self::State) -> Self::Payload;
}
```

- **Associated types:** `Spec` is the input (what to show), `Payload` the
  output snapshot, `Delta` the incremental change, `Key` the dedupe identity,
  `State` the private working set.
- **Lifecycle:** `key` → `dependencies` (registers composite reverse-index
  keys) → `open` (seed state + first payload) → `on_event_*` callbacks return
  `Some(delta)` only when something changed → `snapshot` rebuilds payload.
- **Use it when** the UI needs to observe anything. `ViewDependencies`
  (`view.rs:16-23`) declares `kinds`, `authors`, `ids`, `tag_refs`,
  `projection_keys` — declare them tightly; an empty set forces a table scan
  ([06](06-reactivity-contract.md)).

## ActionModule — durable workflows

`crates/nmp-core/src/substrate/action.rs:10-84`. Writes go through actions;
reads go through views. Never the reverse.

```rust
pub trait ActionModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Action: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Step:   Clone + Serialize + DeserializeOwned + Send + 'static;
    type Output: Clone + Serialize + Send + 'static;

    fn start(ctx: &mut ActionContext, action: Self::Action)
        -> Result<ActionPlan<Self::Step>, ActionRejection>;
    fn reduce(ctx: &mut ActionContext, id: ActionId, input: ActionInput<Self::Step>)
        -> ActionTransition<Self::Step, Self::Output>;
}
// ActionTransition: Continue | Complete | Fail{transient} | AwaitCapability
//                   | AwaitUserApproval        (action.rs:55-77)
// ActionRejection:  Invalid | Unauthorized | Conflict   (action.rs:79-84)
```

- **Lifecycle:** `start` validates and returns an `ActionPlan` (initial step
  + status + optional deadline) or rejects synchronously. `reduce` is the
  step machine, fed `ActionInput` (`Started`, `ResumedAfterRestart`,
  `CapabilityResult`, `RelayOk`, `Timeout`, `Cancel`).
- **Use it when** something mutates state, talks to relays, or needs a
  capability — and must resume cleanly after a restart. The `Result` is
  *internal*; it never crosses FFI (D6).

## CapabilityModule — typed native fact reports

`crates/nmp-core/src/substrate/capability.rs:1-24`.

```rust
pub trait CapabilityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Request: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Result:  Clone + Serialize + DeserializeOwned + Send + 'static;
    fn callback_interface_name() -> &'static str;   // e.g. "FixtureTodoCapability"
}
// wire envelopes: CapabilityRequest / CapabilityEnvelope  (capability.rs:12-24)
//   { namespace, correlation_id, payload_json | result_json }
```

- **Lifecycle:** kernel emits a `CapabilityRequest` → native side executes
  the OS handle → returns a `CapabilityEnvelope` keyed by `correlation_id`.
  Start/stop must be idempotent and safe N times.
- **Use it when** you need an OS handle (keyring, push, audio, network
  monitor). Native code *reports a fact*; it never decides retry, routing, or
  any policy (D7). Results are envelopes, not `Result`-typed errors.

## IdentityModule — signer scopes

`crates/nmp-core/src/substrate/identity.rs:8-76`.

```rust
pub trait IdentityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Descriptor: Clone + Serialize + DeserializeOwned + Send + 'static;

    fn scope_kind() -> IdentityScopeKind;        // Human|AppLocal|External|Ephemeral
    fn create(ctx: &mut IdentityContext, d: Self::Descriptor)
        -> Result<IdentityId, IdentityError>;
    fn sign<'a>(ctx, &'a IdentityId, &'a UnsignedEvent)
        -> BoxFuture<'a, Result<SignedEvent, SigningError>>;
    fn destroy(ctx: &mut IdentityContext, id: &IdentityId);
}
// IdentityScopeKind: HumanAccount | AppLocal | ExternalSigner | Ephemeral
//                                                     (identity.rs:26-32)
```

- **Lifecycle:** `create` mints an `IdentityId` (ctx remembers it) →
  `sign` is async (external signers are remote) → `destroy` releases it.
- **Use it when** you need a signer scope beyond the active Nostr account —
  an app-local agent key, an ephemeral throwaway, an external bunker. Hold
  **no** long-lived state in `IdentityContext`; it tracks created ids only.

## Decision tree: "I want X — which trait?"

```
I want to ...
│
├─ store something durable that is NOT a Nostr event
│     (draft, setting, transcript, weight log)            → DomainModule
│
├─ show something to the UI / observe state reactively   → ViewModule
│     └─ it derives purely from Nostr events                  (deps = kinds/authors/ids/tags)
│     └─ it derives from DomainModule records                 (deps = projection_keys)
│
├─ change state, publish, or run a multi-step workflow
│     that must survive a restart                         → ActionModule
│
├─ ask the OS for a fact (keyring, push, audio, network)  → CapabilityModule
│     (native REPORTS; never decides policy — D7)
│
└─ introduce a signer scope beyond the active account
      (app-local key, ephemeral, external bunker)         → IdentityModule
```

A real app implements several: `fixture-todo-core` implements all five (the
non-Nostr proof); `nmp-nip29` implements Domain + View + Action across 13/7/15
modules. Walkthroughs of app-shaped modules and how they compose via
`ModuleRegistry` are in
[05b](05b-substrate-traits.md).

## Deliverables (this half)

- **Per-family ~15-line shape block** (above) — copy the skeleton, fill the
  associated types, delete the comments.
- **"Which trait?" decision tree** (above) — answer it before opening any
  PR that adds a module.

See also: [02 — Mental model](02-mental-model.md) ·
[05b — Substrate traits: fixture walkthrough + nip29 + composition](05b-substrate-traits.md) ·
[06 — Reactivity contract (D8)](06-reactivity-contract.md) ·
[16 — Capabilities (D7)](16-capabilities.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
