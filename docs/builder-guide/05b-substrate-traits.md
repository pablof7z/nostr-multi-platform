# 05b — Kernel substrate: microblog walkthrough + nip29 + composition

*Status: SHIPS · Audience: both · Read after [05a](05a-substrate-traits.md).*

[05a](05a-substrate-traits.md) gave you the seam signatures and the "which
seam?" tree. This half is the proof the boundary works: an annotated walkthrough
of the `microblog-core` app crate (from [19a](19a-walkthrough-microblog.md)), a
sidebar showing how a real Nostr protocol crate uses the same seams, and how
modules compose through `nmp-defaults`.

## Annotated walkthrough: `microblog-core`

`crates/microblog-core/src/lib.rs` (worked out in
[19a](19a-walkthrough-microblog.md)) is ADR-0009 in practice: an app module
exercising all three extension seams **with app nouns that stay out of
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

### The event observer

```rust
pub struct FeedObserver {
    store: FeedStore,
}

impl KernelEventObserver for FeedObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != 1 { return; }
        let record = NoteRecord {
            id:         event.id.clone(),
            author:     event.author.clone(),
            content:    event.content.clone(),
            created_at: event.created_at,
        };
        if let Ok(mut guard) = self.store.lock() {
            guard.push(record);
        }
    }
}
```

The observer fires on every accepted kind:1 ingest and appends to the feed
store. This is the same seam `nmp-app-chirp` uses to drive the live timeline
projection.

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

This is D8 + D6 in one line: the closure is cheap (one lock + encode), and
panic-safe (returns `null` on mutex poison rather than aborting the snapshot
tick).

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
root: it calls `nmp_defaults::register_defaults(app)` once, then wires
microblog-specific seams.
See [19b](19b-walkthrough-microblog.md) for the shell.

### What `microblog-core` proves

1. A complete app module with writes (`ActionModule`), event-driven view
   (`KernelEventObserver`), and read output (`register_typed_snapshot_projection`)
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
    app.register_action(PostChatMessageAction);
    app.register_action(ReactInGroupAction);
    app.register_action(CreatePublicGroupAction);
    app.register_action(DiscoverGroupsAction);
    app.register_action(JoinGroupAction);
    // … 10 more
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

Every app-core `register()` fn is called once at host init. Under ADR-0046 the
app-core composition root inherits the canonical NMP composition through one
library call, then adds app-specific seams on top. The microblog staticlib shell
does not call defaults itself:

```rust
// apps/microblog/nmp-app-microblog/src/lib.rs
#[no_mangle]
pub extern "C" fn nmp_app_microblog_register(app: *mut NmpApp) {
    if app.is_null() { return; }
    microblog_core::register(unsafe { &mut *app });
}
```

For a headless example, the same composition fits in `examples/shell.rs`
using `NmpAppBuilder`:

```rust
use nmp_defaults::{NmpAppBuilder, RunConfig};

let mut builder = NmpAppBuilder::new();
microblog_core::register(&mut builder);
let app = builder
    .in_memory()
    .declare_consumed_projections(["microblog.items"])
    .start(RunConfig::default());
// Drive the app via `nmp_ffi` symbols, then `nmp_ffi::nmp_app_free(app)`.
```

`nmp_defaults::register_defaults` installs the production routing substrate,
outbox resolver, NIP-02/17/57/65 action modules, DM-inbox + zap-receipts
runtimes, WOT bootstrap, and the NIP-23 long-form typed projection. The app-core
composition root adds only app-specific projections and actions (here, the
microblog feed).

Registration order matters for last-writer-wins slots, but ADR-0049 made the
canonical defaults *yield* to app registrations: an app registering under a
default namespace before or after `register_defaults` wins. App-over-app
namespace collisions remain a bug and are recorded in the composition ledger
(`nmp_app_debug_info(app, domain=1)` — composition-report domain; domain=0 for routing trace, domain=2 for both merged).

## Anti-patterns

1. **App state inside the kernel.** The feed store is an `Arc<Mutex<Vec<…>>>`
   owned by `microblog-core`, not by `nmp-core`. Pushing app records into the
   kernel event store or a kernel-owned map is a D0 violation.
2. **Business policy in a `CapabilityModule` (D7 violation).** A capability
   returns a fact (e.g. keychain has a key). It must not decide retry, routing,
   or "should we publish." Policy lives in the `ActionModule::execute` body.
3. **Blocking inside `register_typed_snapshot_projection`.** The closure runs on the
   actor thread inside every snapshot tick. Any blocking I/O or long-held lock
   stalls all relay ingest behind it (D8 violation). Delegate to a precomputed
   value; the snapshot projector should read, never compute.
4. **Registering the same NAMESPACE twice.** `register_action` accepts the
   second registration silently (last-writer-wins by `namespace` key), but two
   modules sharing a NAMESPACE will race for dispatch. Pick unique dotted
   namespaces per module.
5. **Hand-copying substrate wiring instead of calling `register_defaults`.**
   The shared `Arc<InMemoryMailboxCache>` and coverage gate must reach multiple
   collaborators with the same instance; copying the block by hand desyncs
   them (V-48).
6. **Bypassing the shipped seams.** Use `ActionModule`,
   `KernelEventObserver`, snapshot/typed projection registration, and
   capabilities as shown in [05a](05a-substrate-traits.md).

## Deliverables (this half)

- **Annotated `microblog-core` walkthrough** (above) — the copyable three-seam
  template: ActionModule + event observer + snapshot projection.
- **`nmp-nip29` sidebar** (above) — how the same seams scale to a real
  protocol with zero kernel nouns; plus the ADR-0046 composition pattern.

See also: [02 — Mental model](02-mental-model.md) ·
[05a — Substrate traits: signatures + decision tree](05a-substrate-traits.md) ·
[06 — Reactivity contract (D8)](06-reactivity-contract.md) ·
[16 — Capabilities (D7)](16-capabilities.md) ·
[19a — Walkthrough: build a microblog app (scaffold)](19a-walkthrough-microblog.md) ·
[19b — Walkthrough: build a microblog app (wire & run)](19b-walkthrough-microblog.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
