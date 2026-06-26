# 19a — Walkthrough: build a microblog app (scaffold)

**Status: SHIPS · audience: builders.** Part 1 of 2. This part scaffolds the
hand-written `microblog-core` app crate. [19b](19b-walkthrough-microblog.md)
creates a thin staticlib shell around it, wires the publish path, and runs it
on the iOS simulator.

You are building **a Nostr-shaped app, not a Twitter clone.** The kernel never
learns the word "tweet". kind:1 is the wire; your app projects it into an
app-defined record. That separation *is* the D0 demo — see the callout below.

> **kind:1-shaped, not Twitter-shaped.** No `Tweet`, `Retweet`, or `Like`
> type exists anywhere. The app domain noun is `NoteRecord`; the snapshot
> slice is `microblog.items`. `nmp-core` stays ignorant of every one of these.
> If you find yourself adding `enum Tweet` to `nmp-core`, stop — that is the
> exact D0 violation this walkthrough exists to prevent.

> **Composition model.** This walkthrough uses ADR-0046: a downstream app
> depends on `nmp-defaults` and calls `register_defaults`. See
> [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) for the
> binding and composition split.

## The structural model

Two seams are wired in `register()`: `register_action` for the write path and
`register_snapshot_projection` or `register_typed_snapshot_projection` for the
read path. An `ObservedProjectionSink` feeds
raw kind:1 events into an app-owned feed store. The difference from the old
fixture model is that `register()` is the app-core composition root: it **first
inherits the canonical NMP composition** through
`nmp_defaults::register_defaults(app)` exactly once, then adds the app-specific
seams on top.

The app crate you create for your product should open with the D0 boundary
comment verbatim:

```rust
// D0: app nouns live in app modules, never in nmp-core.
// This crate is the central domain crate for this app.
```

## Complete file tree of the example

```
apps/microblog/
├── nmp.toml                         # AppManifest (used by nmp doctor / upgrade)
└── nmp-app-microblog/               # thin staticlib shell (created in 19b)
    ├── Cargo.toml
    └── src/
        └── lib.rs                   # re-export + register defaults + app register
crates/microblog-core/               # hand-written app-core crate (you write this)
├── Cargo.toml
└── src/
    └── lib.rs                       # records + ActionModule + observer + register()
```

Only `crates/microblog-core/src/lib.rs` and `apps/microblog/nmp.toml` are
hand-written here. The staticlib shell in `apps/microblog/nmp-app-microblog/`
is a few lines of glue (see [19b](19b-walkthrough-microblog.md)).

## `apps/microblog/nmp.toml`

The manifest parser is still read by `nmp doctor` / `nmp upgrade`, so a
minimal `nmp.toml` is useful. It is no longer used to generate a per-app FFI
crate.

```toml
[app]
name = "microblog"
display_name = "NMP Microblog"

[modules]
kernel = "nmp-core"
protocol = []
app = ["microblog-core"]

[platforms]
desktop = true
ios = true
```

> `[platforms]` keys are **silently ignored** by the parser today
> (`manifest.rs` matches only `[app]`/`[modules]`). Do not gate build
> logic on them.

## Records and app-owned state

```rust
// crates/microblog-core/src/lib.rs
// D0: microblog nouns live in this app module, never in nmp-core.
use std::sync::{Arc, Mutex, OnceLock};
use nmp_core::substrate::{AppHost, *};
use serde::{Deserialize, Serialize};

pub const ACTION_NAMESPACE: &str = "microblog.action";
pub const FEED_SNAPSHOT_KEY: &str = "microblog.items";

pub type FeedStore = Arc<Mutex<Vec<NoteRecord>>>;
pub type Store = FeedStore;   // app-core convention name

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteRecord {
    pub id: String,
    pub author: String,   // hex pubkey — no display formatting (aim.md §2)
    pub content: String,
    pub created_at: u64,
}

// Plain projection — cheap read, no actor knowledge.
pub fn project_feed(items: &[NoteRecord]) -> Option<TypedProjectionData> {
    Some(encode_feed_projection(items))
}
```

`NoteRecord.author` is a raw hex pubkey. Formatting (shortened npub, display
name, avatar) is the shell's job (D1 / aim.md §2 anti-patterns).

## ActionModule — posting a note

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
        // Hand the content to the actor. The actor fills pubkey from the
        // active signer, stamps created_at from kernel.now_secs() (D9),
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

> **`is_async_completing() = true`** because the terminal outcome (relay ACK)
> arrives asynchronously through `projections["action_stages"]`. The dispatch
> return carries a `correlation_id`; the host polls `action_stages[id]` for
> `Publishing → Accepted/Failed`.

## ObservedProjectionSink — building the feed

The app builds its feed by implementing `ObservedProjectionSink` and opening a
declared observed projection. Matching kind:1 events fire `on_kernel_event`
after the kernel has replayed cached/store-backed rows and activated scoped
future delivery.

```rust
use nmp_core::{ObservedProjectionSink, KernelEvent};

static FEED_STORE: OnceLock<FeedStore> = OnceLock::new();

pub struct FeedObserver {
    store: FeedStore,
}

impl ObservedProjectionSink for FeedObserver {
    // Fires for every Inserted | Replaced ingest on the actor thread.
    // Duplicates and rejections never reach here.
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != 1 { return; }
        let record = NoteRecord {
            id:         event.id.clone(),
            author:     event.author.clone(),
            content:    event.content.clone(),
            created_at: event.created_at,
        };
        if let Ok(mut guard) = self.store.lock() {
            // Simple append; production would deduplicate + sort by created_at.
            guard.push(record);
        }
    }
}
```

## `register()` — wiring all three seams

```rust
pub fn accepted() -> Update { Update::ActionAccepted }
pub enum Update { ActionAccepted }

pub fn register(app: &mut impl AppHost) -> FeedStore {
    // Initialize the process-wide store once.
    let store: FeedStore = FEED_STORE
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone();

    // 1. Inherit the canonical NMP composition (routing, outbox, DMs, zaps, WOT).
    nmp_defaults::register_defaults(app);

    // 2. Write path.
    app.register_action(NoteActionModule);

    // 3. Event-driven view — declared shape, replay, then scoped delivery.
    app.open_observed_projection(ObservedProjection::from_kinds(
        Arc::new(FeedObserver { store: Arc::clone(&store) }),
        FEED_SNAPSHOT_KEY,
        0,
        [KIND_NOTE],
        128,
    ));

    // 4. Read output — projects the feed into the snapshot.
    let projector = Arc::clone(&store);
    app.register_typed_snapshot_projection(FEED_SNAPSHOT_KEY, move || {
        match projector.lock() {
            Ok(g)  => project_feed(&g),
            Err(_) => None,
        }
    });

    store
}
```

## `crates/microblog-core/Cargo.toml`

```toml
[package]
name = "microblog-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
nmp-core = { path = "../../crates/nmp-core" }
nmp-defaults = { path = "../../crates/nmp-defaults" }
serde      = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
```

`nmp-ffi` is **not** a dependency of the app-core crate. The core writes to
`AppHost` traits; the thin staticlib shell (created in
[19b](19b-walkthrough-microblog.md)) owns the `NmpApp` handle and the C-ABI
surface.

## Next step: the thin staticlib shell

This crate contains the app logic. It has no `#[no_mangle]` symbols and no
iOS-specific code. [19b](19b-walkthrough-microblog.md) wraps it in a
staticlib crate (`apps/microblog/nmp-app-microblog`) whose entire job is to:

1. Link `nmp-defaults`, `nmp-ffi`, and `microblog-core`.
2. Export one registration symbol the iOS shell calls after `nmp_app_new()`.
3. Call `microblog_core::register(app)`. The defaults call is already inside
   that app-core composition root.

That shell is the analog of `nmp_app_chirp_register` in `apps/chirp/crates/nmp-app-chirp`.

## Anti-patterns (scaffold phase)

- **Adding Nostr/Twitter types to `nmp-core`.** `NoteRecord` lives in
  `microblog-core`. The kernel sees raw `KernelEvent`s, never an app noun.
- **Making the example Twitter-shaped.** `Tweet`/`Retweet`/`Like` enums
  defeat the entire D0 demonstration. kind:1 is the wire format; the app
  noun is the only place an app concept appears.
- **Hand-copying substrate wiring instead of calling `register_defaults` or
  `register_substrate`.** The shared `Arc<InMemoryMailboxCache>` and coverage
  gate must reach multiple collaborators with the same instance; copying the
  block by hand desyncs them (V-48).
- **Skipping `open_observed_projection` and rendering raw events in Swift.**
  The feed store is the source of truth; the snapshot projection carries it.
  Raw event arrays across FFI violate D5.
- **Inventing a new extension family.** Use the shipped action, observer,
  projection, capability, and composition seams unless an ADR changes the
  substrate.
- **Expecting generated framework wiring.** The staticlib shell is thin glue
  that calls the app-core composition root.

See also: [02 — Mental model — kernel + extension seams](02-mental-model.md) ·
[05a — Kernel substrate — traits + seams](05a-substrate-traits.md) ·
[12 — Publishing + the publish engine](12-publish-and-ledger.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[19b — Walkthrough: build a microblog app (wire & run)](19b-walkthrough-microblog.md) ·
[20 — Adding a new protocol module](20-new-protocol-module.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
