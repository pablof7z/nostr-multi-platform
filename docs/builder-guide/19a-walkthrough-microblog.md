# 19a — Walkthrough: build a microblog app (scaffold)

**Status: SHIPS · audience: builders.** Part 1 of 2. This part scaffolds the
hand-written `microblog-core` app crate. [19b](19b-walkthrough-microblog.md)
creates a thin binding adapter around it, wires the publish path, and runs it
from a native shell.

You are building **a Nostr-shaped app, not a Twitter clone.** The kernel never
learns the word "tweet". kind:1 is the wire; your app projects it into an
app-defined record. That separation *is* the D0 demo — see the callout below.

> **kind:1-shaped, not Twitter-shaped.** No `Tweet`, `Retweet`, or `Like`
> type exists anywhere. The app domain noun is `NoteRecord`; the snapshot
> slice is `microblog.items`. `nmp-core` stays ignorant of every one of these.
> If you find yourself adding `enum Tweet` to `nmp-core`, stop — that is the
> exact D0 violation this walkthrough exists to prevent.

> **Composition model.** This walkthrough uses ADR-0069: a downstream app owns
> an explicit Rust composition root. `nmp-substrate` provides the shared
> substrate floor; protocol and app features are installed by their owner
> crates. Hidden presets and replacement defaults bundles are not production,
> tutorial, migration, or test architecture. See
> [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md).

## The structural model

The example shows low-level seams: `register_action` for the write path and
typed output for the read path. In production, product screens should open typed
read sessions or generated helpers; raw observed delivery is executor machinery
behind those sessions. `register()` is the app-core composition root: it installs
explicit substrate/protocol/app features, then adds the app-specific seams.

The app crate you create for your product should open with the D0 boundary
comment verbatim:

```rust
// D0: app nouns live in app modules, never in nmp-core.
// This crate is the central domain crate for this app.
```

## Complete file tree of the example

```
apps/microblog/
├── nmp.toml                         # AppManifest (used by nmp upgrade)
└── nmp-app-microblog/               # thin binding adapter (created in 19b)
    ├── Cargo.toml
    └── src/
        └── lib.rs                   # re-export + app register
crates/microblog-core/               # hand-written app-core crate (you write this)
├── Cargo.toml
└── src/
    └── lib.rs                       # records + ActionModule + read-session helper + register()
```

Only `crates/microblog-core/src/lib.rs` and `apps/microblog/nmp.toml` are
hand-written here. The binding adapter in `apps/microblog/nmp-app-microblog/`
is a few lines of glue (see [19b](19b-walkthrough-microblog.md)).

## `apps/microblog/nmp.toml`

The manifest parser is still read by `nmp upgrade`, so a minimal `nmp.toml` is
useful. It is no longer used to generate a per-app FFI crate.

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
use nmp_core::substrate::{AppHost, *};
use serde::{Deserialize, Serialize};

pub const ACTION_NAMESPACE: &str = "microblog.action";
pub const FEED_KEY: &str = "microblog.timeline.home";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteRecord {
    pub id: String,
    pub author: String,   // hex pubkey — no display formatting (aim.md §2)
    pub content: String,
    pub created_at: u64,
}
```

`NoteRecord.author` is a raw hex pubkey. Formatting (shortened npub, display
name, avatar) is the shell's job (D1 / aim.md §2 anti-patterns). `NoteRecord`
is the app-owned decoded shape for one feed row; the production feed path
(below, `app.feeds().open_spec`) delivers rows through the standard
`FeedParams`/`FeedItemProjection` contract — the app never hand-rolls a
snapshot store or a projection closure to get notes onto the timeline.

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
        _ctx: &ActionContext,
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
> return carries a `correlation_id`; the host observes `action_stages[id]` for
> `Publishing → Accepted/Failed`.

## The feed — `app.feeds().open_spec`

The microblog timeline is a feed-shaped read: kind:1 notes over the active
account's follows. That is exactly what the production app-facing feed helper
covers (ADR-0076; [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md)
has the canonical `open_spec` example this walkthrough mirrors; [07 —
Subscription planner](07-subscription-planner.md) covers the underlying
session), so this walkthrough opens the feed directly instead of hand-rolling
a store and a projection closure:

```rust
pub fn open_home_feed(app: &impl AppHost) -> Result<FeedHandle, FeedSpecOpenError> {
    app.feeds().open_spec(
        FeedKey::app(FEED_KEY)?,
        feed::events()
            .primary_kinds([1])
            .from(source::active_user().follows())
            .shape(FeedShape::RootIndexed)
            .order(FeedOrder::NewestByFeedPosition)
            .window(FeedWindowPolicy::bounded(80))
            .project(FeedItemProjection::feed_rows()),
    )
}
```

`open_spec` compiles the descriptor into canonical `FeedParams`, opens the
session through the standard NMP feed compiler, and returns a `FeedHandle`
the shell holds for pagination (`app.feeds().load_older(&handle)`) and
teardown (`app.feeds().close(&handle)`). Compiler selection, observer
registration, replay-before-live sequencing, source reconciliation (follows
change on account switch), and typed output all stay internal runtime
machinery behind that one call — the app never touches them directly.

Each pushed row decodes (host-side, via the generated typed-output helper)
into the app-owned `NoteRecord` declared above.

> **Sidebar: toy projection shape (for illustrating projections only — NOT
> the feed API).** Before `app.feeds().open_spec` existed, this walkthrough
> demonstrated the underlying typed-output projection mechanism with a
> hand-rolled `Arc<Mutex<Vec<NoteRecord>>>` store and a manual
> `register_typed_snapshot_projection` closure. That shape is useful for
> understanding *what a typed projection is* — a plain function from
> app-owned state to `Option<TypedProjectionData>`, installed once — but it
> is not how a real feed is built. Do not copy it as a feed implementation:
>
> ```rust
> use std::sync::{Arc, Mutex, OnceLock};
>
> pub type FeedStore = Arc<Mutex<Vec<NoteRecord>>>;
> static FEED_STORE: OnceLock<FeedStore> = OnceLock::new();
>
> pub fn project_feed(items: &[NoteRecord]) -> Option<TypedProjectionData> {
>     Some(encode_feed_projection(items))
> }
>
> pub fn register_microblog_toy_projection(app: &mut impl AppHost, store: FeedStore) {
>     let projector = Arc::clone(&store);
>     app.register_typed_snapshot_projection("microblog.items", move || {
>         match projector.lock() {
>             Ok(g) => project_feed(&g),
>             Err(_) => None,
>         }
>     });
> }
> ```
>
> `register()` below does not call this helper — it opens the feed through
> `open_home_feed` instead.

## `register()` — wiring the app root

```rust
pub fn accepted() -> Update { Update::ActionAccepted }
pub enum Update { ActionAccepted }

pub fn register(app: &mut impl AppHost) -> Result<FeedHandle, FeedSpecOpenError> {
    // 1. Install explicit substrate/protocol features.
    let _substrate_handles =
        nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());
    nmp_nip50::register(app, nmp_nip50::Config::default())?;
    nmp_nip02::register(app, nmp_nip02::Config::default())?;
    nmp_replies::register(app, nmp_replies::Config::default())?;
    nmp_nip17::register(app, nmp_nip17::Config::default())?;
    nmp_nip22::register(app, nmp_nip22::Config::default())?;
    nmp_nip23::register(app, nmp_nip23::Config::default())?;

    // 2. Write path.
    app.register_action(NoteActionModule);

    // 3. Read path — the standard feed helper owns demand, replay, source
    // reconciliation, output, and teardown behind the returned handle.
    open_home_feed(app)
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
nmp-substrate = { path = "../../crates/nmp-substrate" }
nmp-content = { path = "../../crates/nmp-content" }
nmp-nip02 = { path = "../../crates/nmp-nip02" }
nmp-nip17 = { path = "../../crates/nmp-nip17" }
nmp-nip22 = { path = "../../crates/nmp-nip22" }
nmp-nip50 = { path = "../../crates/nmp-nip50" }
nmp-replies = { path = "../../crates/nmp-replies" }
serde      = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
```

Binding crates are **not** dependencies of the app-core crate. The core writes
to `AppHost` traits; `nmp-native-runtime` owns the runtime builder, while native
UniFFI and browser wasm-bindgen adapters expose the binding surface.

## Next step: the thin binding adapter

This crate contains the app logic. It has no `#[no_mangle]` symbols and no
iOS-specific code. [19b](19b-walkthrough-microblog.md) wraps it in a binding
adapter (`apps/microblog/nmp-app-microblog`) whose entire job is to:

1. Link `nmp-substrate`, selected protocol crates, `nmp-native-runtime`, the
   binding adapter, and `microblog-core`.
2. Expose one configuration function the generated binding calls before start.
3. Call `microblog_core::register(app)`. The named substrate/protocol/app
   installers are already inside that app-core composition root.

Historical native registration symbols are transitional compatibility glue,
not the recipe for new apps.

## Anti-patterns (scaffold phase)

- **Adding Nostr/Twitter types to `nmp-core`.** `NoteRecord` lives in
  `microblog-core`. The kernel sees raw `KernelEvent`s, never an app noun.
- **Making the example Twitter-shaped.** `Tweet`/`Retweet`/`Like` enums
  defeat the entire D0 demonstration. kind:1 is the wire format; the app
  noun is the only place an app concept appears.
- **Hand-copying substrate wiring instead of using reusable installers.** Shared
  collaborators such as mailbox caches and coverage gates must reach multiple
  collaborators with the same instance; copying the block by hand desyncs them.
- **Rendering raw events in Swift/Kotlin/TypeScript.** Rust-owned typed read
  output is the source of truth. Raw event arrays across FFI violate D5.
- **Exposing observed delivery as the app API.** ADR-0070 makes typed read
  sessions/helpers the app-facing read model. Observed-delivery executor
  machinery is internal/protocol-substrate machinery unless a
  later ADR says otherwise.
- **Hand-rolling a feed store.** A feed-shaped read (kind:N over an
  author/follows/list/relay-set source) always opens through
  `app.feeds().open_spec` / `open_feed` (ADR-0076). An `Arc<Mutex<Vec<_>>>`
  plus a manual `register_typed_snapshot_projection` closure reinvents
  acquisition, replay-before-live, pagination, and source reconciliation that
  the feed engine already owns — see the sidebar above for why that shape is
  teaching-only, never product code.
- **Inventing a new extension family.** Use the shipped action, typed read
  output, capability, and composition seams unless an ADR changes the substrate.
- **Expecting generated framework wiring.** The staticlib shell is thin glue
  that calls the app-core composition root.

See also: [02 — Mental model — kernel + extension seams](02-mental-model.md) ·
[05a — Kernel substrate — traits + seams](05a-substrate-traits.md) ·
[12 — Publishing + the publish engine](12-publish-and-ledger.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[19b — Walkthrough: build a microblog app (wire & run)](19b-walkthrough-microblog.md) ·
[20 — Adding a new protocol module](20-new-protocol-module.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
