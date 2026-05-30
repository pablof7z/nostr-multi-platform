# 19a — Walkthrough: build a microblog app (scaffold)

**Status: SHIPS · audience: builders.** Part 1 of 2. This part scaffolds a
kind:1 microblog app-core crate and its per-app FFI crate.
[19b](19b-walkthrough-microblog.md) wires codegen, the publish path, and the
iOS shell.

You are building **a Nostr-shaped app, not a Twitter clone.** The kernel never
learns the word "tweet". kind:1 is the wire; your app projects it into an
app-defined record. That separation *is* the D0 demo — see the callout below.

> **kind:1-shaped, not Twitter-shaped.** No `Tweet`, `Retweet`, or `Like`
> type exists anywhere. The app domain noun is `NoteRecord`; the snapshot
> slice is `microblog.items`. `nmp-core` stays ignorant of every one of these.
> If you find yourself adding `enum Tweet` to `nmp-core`, stop — that is the
> exact D0 violation this walkthrough exists to prevent.

## The structural model

This walkthrough mirrors `apps/fixture/fixture-todo-core/src/lib.rs` (the
canonical reference) in structure. Two seams are wired in `register()`:
`register_action` for the write path and `register_snapshot_projection` for
the read path. A `KernelEventObserver` feeds raw kind:1 events into an
app-owned feed store.

The real "non-fixture" app crate you create for your product should open
with the D0 boundary comment verbatim:

```rust
// D0: app nouns live in app modules, never in nmp-core.
// This crate is the central domain crate for this app.
```

## Complete file tree of the example

```
apps/microblog/
├── nmp.toml                         # AppManifest (5 lines)
└── nmp-app-microblog/               # generated per-app FFI crate
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── action.rs                # AppAction enum
        ├── update.rs                # AppUpdate enum
        ├── envelope.rs              # envelope helpers
        ├── view_spec.rs             # ViewSpec enum
        ├── ffi.rs                   # FfiApp dispatch shell
        ├── domain.rs
        └── capability.rs
crates/microblog-core/              # hand-written app-core crate (you write this)
├── Cargo.toml
└── src/
    └── lib.rs                       # records + ActionModule + observer + register()
```

Only `crates/microblog-core/src/lib.rs` and `apps/microblog/nmp.toml` are
hand-written. Everything under `nmp-app-microblog/src/` is codegen output —
never hand-edit it.

## `apps/microblog/nmp.toml`

Mirrors `apps/fixture/nmp.toml` field for field:

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
use nmp_core::substrate::*;
use nmp_ffi::NmpApp;
use serde::{Deserialize, Serialize};

pub const ACTION_NAMESPACE: &str = "microblog.action";
pub const FEED_SNAPSHOT_KEY: &str = "microblog.items";

pub type FeedStore = Arc<Mutex<Vec<NoteRecord>>>;
pub type Store = FeedStore;   // codegen convention name

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteRecord {
    pub id: String,
    pub author: String,   // hex pubkey — no display formatting (aim.md §2)
    pub content: String,
    pub created_at: u64,
}

// Plain projection — cheap read, no actor knowledge.
pub fn project_feed(items: &[NoteRecord]) -> serde_json::Value {
    serde_json::json!({ "notes": items })
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

## KernelEventObserver — building the feed

The app builds its feed by implementing `KernelEventObserver`. Every accepted
kind:1 event fires `on_event_inserted`; the observer appends it to the store.

```rust
use nmp_core::{KernelEventObserver, KernelEvent};

static FEED_STORE: OnceLock<FeedStore> = OnceLock::new();

pub struct FeedObserver {
    store: FeedStore,
}

impl KernelEventObserver for FeedObserver {
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
pub enum ViewSpec {}
pub enum Update { ActionAccepted }

pub fn register(app: &mut NmpApp) -> FeedStore {
    // Initialize the process-wide store once.
    let store: FeedStore = FEED_STORE
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone();

    // Seam 1: write path.
    app.register_action::<NoteActionModule>();

    // Seam 2: event-driven view — populates the feed store on every ingest.
    app.register_event_observer(Arc::new(FeedObserver { store: Arc::clone(&store) }));

    // Seam 3: read output — projects the feed into the snapshot.
    let projector = Arc::clone(&store);
    app.register_snapshot_projection(FEED_SNAPSHOT_KEY, move || {
        match projector.lock() {
            Ok(g)  => project_feed(&g),
            Err(_) => serde_json::Value::Null,
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
nmp-ffi  = { path = "../../crates/nmp-ffi" }
serde      = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
```

## Per-app FFI crate skeleton

`nmp-app-microblog/src/{lib,action,update,view_spec,ffi}.rs` are codegen
output. `action.rs` will look like:

```rust
// GENERATED by `nmp gen modules` — do not hand-edit.
#[derive(Clone, Debug, PartialEq)]
pub enum AppAction {
    Kernel(nmp_core::KernelAction),
    MicroblogCore(microblog_core::Action),
}
```

And `ffi.rs` calls `microblog_core::register(unsafe { &mut *app })` in
`FfiApp::new`, stores the returned `Store`, and routes
`AppAction::MicroblogCore(a)` through `dispatch_app_action(ACTION_NAMESPACE, …)`.
These files are regenerated; edits are lost. See
[19b](19b-walkthrough-microblog.md) for how to run codegen.

## Anti-patterns (scaffold phase)

- **Adding Nostr/Twitter types to `nmp-core`.** `NoteRecord` lives in
  `microblog-core`. The kernel sees raw `KernelEvent`s, never an app noun.
- **Making the example Twitter-shaped.** `Tweet`/`Retweet`/`Like` enums
  defeat the entire D0 demonstration. kind:1 is the wire format; the app
  noun is the only place an app concept appears.
- **Hand-editing the per-app FFI crate.** `nmp-app-microblog/src/*` is
  regenerated; edits are lost and break the codegen determinism test.
- **Skipping `register_event_observer` and rendering raw events in Swift.**
  The feed store is the source of truth; the snapshot projection carries it.
  Raw event arrays across FFI violate D5.
- **Using the removed `ViewModule` / `DomainModule` traits.** They are not on
  master — see [05a](05a-substrate-traits.md) §Removed v2 traits.

See also: [02 — Mental model — kernel + extension seams](02-mental-model.md) ·
[05a — Kernel substrate — traits + seams](05a-substrate-traits.md) ·
[15 — Codegen — `nmp gen modules` + per-app FFI crate](15-codegen-and-ffi.md) ·
[19b — Walkthrough: build a microblog app (wire & run)](19b-walkthrough-microblog.md) ·
[20 — Adding a new protocol module](20-new-protocol-module.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
