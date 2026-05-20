# 19a — Walkthrough: build a microblog app (scaffold)

**Status: SHIPS · audience: builders.** Part 1 of 2. This part scaffolds a
kind:1 microblog app-core crate and its per-app FFI crate. [19b](19b-walkthrough-microblog.md)
wires codegen, the publish path, and the iOS shell.

You are building **a Nostr-shaped app, not a Twitter clone.** The kernel never
learns the word "tweet". kind:1 is the wire; your app projects it into an
app-defined record. That separation *is* the D0 demo — see the callout below.

> **kind:1-shaped, not Twitter-shaped.** No `Tweet`, `Retweet`, or `Like`
> type exists anywhere. The app domain noun is `NoteRecord`; the view is
> `MicroblogFeed`. `nmp-core` stays ignorant of every one of these. If you
> find yourself adding `enum Tweet` to `nmp-core`, stop — that is the exact
> D0 violation this walkthrough exists to prevent.

## The structural model

This example mirrors [`crates/fixture-todo-core/src/lib.rs:1-305`](../../crates/fixture-todo-core/src/lib.rs)
(the canonical 5-family example) structurally, and the per-app FFI crate
mirrors [`apps/fixture/nmp-app-fixture/`](../../apps/fixture/nmp-app-fixture)
exactly. The real "non-fixture" app crate to compare against is the future
app-owned crate you create for your product, which should open with the D0
boundary comment verbatim:

```rust
// D0: app nouns live in app modules, never in nmp-core.
// This crate is the central domain crate for this app.
```

Your microblog crate carries the same comment with `app` -> `microblog`.

## Complete file tree of the example

```
apps/microblog/
├── nmp.toml                         # AppManifest (5 lines)
└── nmp-app-microblog/               # generated per-app FFI crate
    ├── Cargo.toml
    └── src/
        ├── lib.rs                   # re-exports AppAction/AppUpdate/ViewSpec/FfiApp
        ├── action.rs                # AppAction enum (Kernel | MicroblogCore)
        ├── update.rs                # AppUpdate enum
        ├── view_spec.rs             # ViewSpec enum
        ├── ffi.rs                   # FfiApp dispatch shell
        ├── domain.rs                # generated marker
        └── capability.rs            # generated marker
crates/microblog-core/              # hand-written app-core crate (you write this)
├── Cargo.toml
└── src/
    └── lib.rs                       # Domain + View + Action modules
```

Only `crates/microblog-core/src/lib.rs` and `apps/microblog/nmp.toml` are
hand-written. Everything under `nmp-app-microblog/src/` is codegen output
(see [19b](19b-walkthrough-microblog.md)) — never hand-edit it.

## `apps/microblog/nmp.toml`

Mirrors [`apps/fixture/nmp.toml`](../../apps/fixture/nmp.toml) field for field:

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

## `crates/microblog-core/src/lib.rs` — Domain module (~30 lines)

A `NoteRecord` is the app's projection of a kind:1 event. The kernel stores
the raw event (D4); this domain record is what the view materializes.

```rust
// D0: microblog nouns live in this app module, never in nmp-core.
use nmp_core::substrate::*;
use serde::{Deserialize, Serialize};

pub const APP_MODULE: &str = "microblog";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteRecord {
    pub id: String,         // event id
    pub author: String,     // pubkey hex
    pub text: String,       // kind:1 content, verbatim
    pub created_at: u64,
}

pub struct NoteDomainModule;

impl DomainModule for NoteDomainModule {
    const NAMESPACE: &'static str = "microblog.domain";
    const SCHEMA_VERSION: u32 = 1;

    fn migrations() -> Vec<DomainMigration> { Vec::new() }

    fn indexes() -> Vec<DomainIndex> {
        vec![DomainIndex {
            name: "by_author",
            key_fn: |bytes| serde_json::from_slice::<NoteRecord>(bytes)
                .ok().map(|n| n.author.into_bytes()),
        }]
    }

    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<NoteRecord>();
    }
}
```

## `crates/microblog-core/src/lib.rs` — View module (~30 lines)

`MicroblogFeed` is the bounded snapshot the UI renders. It follows
`fixture-todo-core`'s `TodoViewModule` shape exactly: `Spec` → `Key`,
`on_projection_changed` emits the replace delta.

```rust
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct FeedSpec { pub author_filter: Option<String> }

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MicroblogFeed { pub notes: Vec<NoteRecord> }

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FeedDelta { Replaced { payload: MicroblogFeed } }

#[derive(Clone, Debug, Default)]
pub struct FeedState { payload: MicroblogFeed }

pub struct FeedViewModule;

impl ViewModule for FeedViewModule {
    const NAMESPACE: &'static str = "microblog.view";
    type Spec = FeedSpec;
    type Payload = MicroblogFeed;
    type Delta = FeedDelta;
    type Key = Option<String>;
    type State = FeedState;

    fn key(spec: &Self::Spec) -> Self::Key { spec.author_filter.clone() }
    fn dependencies(_s: &Self::Spec) -> ViewDependencies { ViewDependencies::default() }
    fn open(_c: &ViewContext, _s: Self::Spec) -> (Self::State, Self::Payload) {
        let p = MicroblogFeed::default();
        (FeedState { payload: p.clone() }, p)
    }
    fn on_projection_changed(_c: &ViewContext, st: &mut Self::State,
        _ch: &ProjectionChange) -> Option<Self::Delta> {
        Some(FeedDelta::Replaced { payload: st.payload.clone() })
    }
    fn snapshot(_c: &ViewContext, st: &Self::State) -> Self::Payload {
        st.payload.clone()
    }
}
```

(`on_event_inserted` / `_removed` / `_replaced` follow the same `None`-returning
shape as `fixture-todo-core/src/lib.rs:89-112` until you populate the
projection — omitted here for length; copy them verbatim.)

## `crates/microblog-core/src/lib.rs` — Action module (~30 lines)

The post action validates locally then defers to the kernel publish engine.
It does **not** sign or build the event itself — that wiring is in
[19b](19b-walkthrough-microblog.md).

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Action { PostNote { text: String } }

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum NoteStep { BuildAndPublish }

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ActionOutput { Queued }

pub struct NoteActionModule;

impl ActionModule for NoteActionModule {
    const NAMESPACE: &'static str = "microblog.action";
    type Action = Action;
    type Step = NoteStep;
    type Output = ActionOutput;

    fn start(_c: &mut ActionContext, a: Self::Action)
        -> Result<ActionPlan<Self::Step>, ActionRejection> {
        let Action::PostNote { text } = &a;
        if text.trim().is_empty() {
            return Err(ActionRejection::Invalid("empty note".into()));
        }
        Ok(ActionPlan {
            initial_step: NoteStep::BuildAndPublish,
            initial_status: ActionStatus::Running,
            deadline_ms: None,
        })
    }

    fn reduce(_c: &mut ActionContext, _id: ActionId,
        _i: ActionInput<Self::Step>) -> ActionTransition<Self::Step, Self::Output> {
        ActionTransition::Complete { output: ActionOutput::Queued }
    }
}

pub fn module_descriptors() -> ModuleRegistry {
    let mut r = ModuleRegistry::default();
    r.register_domain::<NoteDomainModule>();
    r.register_view::<FeedViewModule>();
    r.register_action::<NoteActionModule>();
    r
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
nmp-core = { path = "../nmp-core" }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
```

## Per-app FFI crate skeleton

`nmp-app-microblog/src/{lib,action,update,view_spec,ffi}.rs` mirror
[`apps/fixture/nmp-app-fixture/src/`](../../apps/fixture/nmp-app-fixture/src)
1:1 with `FixtureTodoCore` → `MicroblogCore` and `fixture_todo_core` →
`microblog_core`. For example `action.rs`:

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum AppAction {
    Kernel(nmp_core::KernelAction),
    MicroblogCore(microblog_core::Action),
}
```

`ffi.rs` carries the same `rev`-bumping `dispatch` shell as
[`nmp-app-fixture/src/ffi.rs:17-22`](../../apps/fixture/nmp-app-fixture/src/ffi.rs).
These five files are **codegen output**, shown here only so you recognize the
shape — see [19b](19b-walkthrough-microblog.md) for how they are produced and
why you must never hand-edit them.

## Anti-patterns (scaffold phase)

- **Adding Nostr/Twitter types to `nmp-core`.** `NoteRecord` lives in
  `microblog-core`. The kernel sees kind:1 events, never an app noun.
- **Making the example Twitter-shaped.** `Tweet`/`Retweet`/`Like` enums
  defeat the entire D0 demonstration. kind:1 is the wire format; the app
  projection is the only place an app noun appears.
- **Hand-editing the per-app FFI crate.** `nmp-app-microblog/src/*` is
  regenerated; edits are lost and break the codegen determinism test.
- **Skipping the ViewModule and rendering raw events in SwiftUI.** The
  bounded `MicroblogFeed` snapshot is the contract; raw event arrays across
  FFI violate D5.

See also: [02 — Mental model — kernel + 5 trait families](02-mental-model.md) ·
[05 — Kernel substrate — the 5 trait families](05-substrate-traits.md) ·
[15 — Codegen — `nmp gen modules` + per-app FFI crate](15-codegen-and-ffi.md) ·
[19b — Walkthrough: build a microblog app (wire & run)](19b-walkthrough-microblog.md) ·
[20 — Adding a new protocol module](20-new-protocol-module.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
