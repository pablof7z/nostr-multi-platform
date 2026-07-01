# 05b — Kernel substrate: microblog walkthrough + nip29 + composition

*Status: SHIPS · Audience: both · Read after [05a](05a-substrate-traits.md).*

[05a](05a-substrate-traits.md) gave you the seam signatures and the "which
seam?" tree. This half is the proof the boundary works: an annotated walkthrough
of a small app crate, a sidebar showing how a real Nostr protocol crate uses the
same Rust seams, and how modules compose through an explicit app root.

## Annotated walkthrough: `microblog-core`

`crates/microblog-core/src/lib.rs` (worked out in
[19a](19a-walkthrough-microblog.md)) is ADR-0009 in practice: an app module
exercising the public app seams **with app nouns that stay out of
`nmp-core`**. It is the canonical template — read it before writing any module.

### The record type

```rust
// crates/microblog-core/src/lib.rs
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteRecord {
    pub id: String,
    pub author: String,   // hex pubkey — no display formatting (aim.md §2)
    pub content: String,
    pub created_at: u64,
}
```

The kernel never sees `NoteRecord`. It is an app noun that lives entirely in
this crate (D0). The kernel sees only typed action payload bytes and projection
bytes crossing the registered seams.

### The action enum and ActionModule

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Action { PostNote { text: String } }

pub struct NoteActionModule;

impl ActionModule for NoteActionModule {
    const NAMESPACE: &'static str = "microblog.action";
    type Action = Action;

    fn start(_ctx: &mut ActionContext, a: Self::Action)
        -> Result<(), ActionRejection> {
        let Action::PostNote { text } = &a;
        if text.trim().is_empty() {
            return Err(ActionRejection::Invalid("empty note".into()));
        }
        Ok(())
    }

    fn is_async_completing() -> bool { true }  // relay ack arrives later

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> {
        let Action::PostNote { text } = action;
        // Hand the content to the actor. The actor fills pubkey, timestamp,
        // signs, and routes via NIP-65 outbox (D3). App never picks relays.
        send(nmp_core::ActorCommand::PublishNote {
            content: text,
            reply_to_id: None,
            target: nmp_core::publish::PublishTarget::Auto,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}
```

Key teaching points:
- `start` rejects bad input *synchronously*. The executor never runs.
- `execute` dispatches an `ActorCommand` for Nostr-shaped work. A purely local
  action would use `_send` and mutate its own store instead.
- `is_async_completing() = true` because the terminal outcome (relay ACK)
  arrives later through `projections["action_stages"]`.
- App state (`FEED_STORE`) is a `static OnceLock<Arc<Mutex<Vec<NoteRecord>>>>`.
  The `execute` body is a static method (no `&self`), so it reads the store
  from the process-wide slot that `register()` initializes.

### The concept-owned active read

`microblog-core` owns the "home feed" concept. It exposes a concrete
`open_microblog_feed` helper (returning a close handle) and registers the
executor behind it. Product screens and native shells call that named helper —
they never spell a generic claim/release verb or a `open_session(namespace,
bytes)` doorway ([#2508](https://github.com/pablof7z/nostr-multi-platform/issues/2508)).

```rust
pub fn register_microblog_feed_concept(app: &mut impl AppHost, store: FeedStore) {
    // Shape only:
    // - demand: kind:1 notes
    // - replay: bounded before live activation
    // - output: FEED_SNAPSHOT_KEY typed sidecar
    // - close: drop the returned handle → internal owner released, output cleared/tombstoned
    //
    // The concept helper may use observed delivery and internal refcounting
    // privately. Native shells call open_microblog_feed() and drop the handle
    // to close; they render its typed output.
    install_feed_executor(app, store);
}
```

The concept owner is the app-facing read contract. It owns acquisition,
replay-before-live, scoped delivery, typed output, status, and teardown.
Observed delivery, source reduction, internal refcounting, and raw interest
materialization are private executor details behind the concept helper.

### The snapshot projection

```rust
pub fn project_feed(items: &[NoteRecord]) -> Option<TypedProjectionData> {
    Some(encode_feed_projection(items))
}

// In register():
let projector = Arc::clone(&store);
app.register_typed_snapshot_projection(FEED_SNAPSHOT_KEY, move || {
    match projector.lock() {
        Ok(g)  => project_feed(&g),
        Err(_) => None,   // D6: no panic on poison
    }
});
```

This is D8 + D6 in one line: the producer is cheap and panic-safe. It is output
transport machinery, not the public read lifecycle.

### The codegen convention exports

```rust
pub const ACTION_NAMESPACE: &str = NoteActionModule::NAMESPACE;
pub const FEED_SNAPSHOT_KEY: &str = "microblog.items";
pub type FeedStore = Arc<Mutex<Vec<NoteRecord>>>;
pub type Store = FeedStore;   // app-core convention alias

pub fn register(app: &mut impl AppHost) -> FeedStore { /* seam wiring */ }
pub fn accepted() -> Update { Update::ActionAccepted }

pub enum Update { ActionAccepted }
```

The thin staticlib shell (`apps/microblog/nmp-app-microblog/src/lib.rs`) calls
`microblog_core::register(app)`. That app-core function is the composition
root: it installs the substrate/protocol features it needs, then wires
microblog-specific actions and concept-owned reads.
See [19b](19b-walkthrough-microblog.md) for the shell.

### What `microblog-core` proves

1. A complete app module with writes (`ActionModule`), a concept-owned active
   read, and typed read output
   — **without touching `nmp-core`**.
2. App state is app-owned (`Arc<Mutex<Vec<NoteRecord>>>`). The kernel never
   stores, migrates, or indexes `NoteRecord`. The module owns its data.
3. Validation is synchronous (`start`). Nostr-publishing actions are
   async-completing (`is_async_completing()`); local-only work would be
   synchronous.

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

Registration (`crates/nmp-nip29/src/register.rs`):

```rust
pub fn register_actions(app: &mut NmpApp) {
    app.register_action(PublishGroupEventAction);
    app.register_action(CreateGroupAction);
    app.register_action(DiscoverGroupsAction);
    app.register_action(JoinGroupAction);
    // additional NIP-29 owner actions
}
```

And the snapshot projector:

```rust
app.register_typed_snapshot_projection("nmp.nip29.group_chat", move || {
    projection.typed_snapshot()  // non-blocking read-model snapshot
});
```

The crate-boundary statement at `lib.rs:10–19` is the doctrine in code:
*`nmp-nip29` does NOT import any other `nmp-nip*` crate; cross-protocol
composition happens at the app layer; the only generic surface added to
`nmp-core` is the third routing lane (`relay_pin` + lattice Rule 9).*

`nmp-nip29` uses `action/` for `ActionModule`s and `projection/` for its read
model.

## Module composition: app-core `register()`

Every app-core `register()` fn is called once at host init. Under ADR-0069 the
app-core composition root installs explicit substrate, protocol, app, and
capability features. The microblog staticlib shell does not compose NMP itself:

```rust
// Native binding adapter shape.
pub fn configure_microblog(app: &mut impl AppHost) {
    microblog_core::register(app);
}
```

For a headless example, the same composition fits in `examples/shell.rs`
using the native runtime builder:

```rust
// Shape only: exact builder names are owned by the live runtime crates.
use nmp_native_runtime::{NmpAppBuilder, RunConfig};

let mut builder = NmpAppBuilder::new();
microblog_core::register(&mut builder);
let app = builder
    .in_memory()
    .with_typed_output_contract(["microblog.items"])
    .without_initial_relays()
    .start(RunConfig::default());
// UniFFI native bindings and wasm-bindgen browser bindings drive the same
// runtime through typed byte-action and update-frame doorways.
```

Reusable installers may provide routing substrate, outbox resolver, protocol
actions, runtime helpers, WOT bootstrap, and typed protocol outputs. The
production app root chooses those installers explicitly and keeps app-specific
policy in the app crate.

Registration order matters for last-writer-wins slots, but ADR-0049 made
reusable installers *yield* to app registrations. App-over-app namespace
collisions remain a bug and are recorded in the composition ledger exposed by
the diagnostics binding (composition-report domain; routing trace remains a
separate diagnostic domain).

## Anti-patterns

1. **App state inside the kernel.** The feed store is an `Arc<Mutex<Vec<…>>>`
   owned by `microblog-core`, not by `nmp-core`. Pushing app records into the
   kernel event store or a kernel-owned map is a D0 violation.
2. **Business policy in a `CapabilityModule` (D7 violation).** A capability
   returns a fact (e.g. keychain has a key). It must not decide retry, routing,
   or "should we publish." Policy lives in the `ActionModule::execute` body.
3. **Blocking inside typed output producers.** They run on the actor update path.
   Any blocking I/O or long-held lock stalls the host update stream (D8
   violation). Delegate to a precomputed value; output producers should read,
   never perform slow work.
4. **Registering the same NAMESPACE twice.** `register_action` accepts the
   second registration silently (last-writer-wins by `namespace` key), but two
   modules sharing a NAMESPACE will race for dispatch. Pick unique dotted
   namespaces per module.
5. **Hand-copying substrate wiring instead of using reusable installers.**
   Shared collaborators such as mailbox caches and coverage gates must be
   installed once and passed by the composition root. Copying wiring blocks by
   hand desyncs them.
6. **Bypassing the shipped seams.** Use typed actions, concept-owned active
   reads, typed output, and capabilities as shown in [05a](05a-substrate-traits.md).

## Deliverables (this half)

- **Annotated `microblog-core` walkthrough** (above) — a low-level seam proof:
  ActionModule + internal observed delivery + typed output.
- **`nmp-nip29` sidebar** (above) — how the same seams scale to a real
  protocol with zero kernel nouns; plus the ADR-0046 composition pattern.

See also: [02 — Mental model](02-mental-model.md) ·
[05a — Substrate traits: signatures + decision tree](05a-substrate-traits.md) ·
[06 — Reactivity contract (D8)](06-reactivity-contract.md) ·
[16 — Capabilities (D7)](16-capabilities.md) ·
[19a — Walkthrough: build a microblog app (scaffold)](19a-walkthrough-microblog.md) ·
[19b — Walkthrough: build a microblog app (wire & run)](19b-walkthrough-microblog.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
