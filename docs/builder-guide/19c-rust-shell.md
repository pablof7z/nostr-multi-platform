# 19c — Rust-native shell: bootstrapping the kernel from Rust

**Status: SHIPS** · Audience: builders · Dependency: [19a](19a-walkthrough-microblog.md), [19b](19b-walkthrough-microblog.md)

This section answers the question the walkthrough leaves open: *how do I actually
start the NMP kernel from a Rust binary?* Use this when your host is a Rust TUI,
a headless test harness, a CLI tool, or any Rust process — not an iOS/Android
native shell.

## The entry point: `NmpAppBuilder`

`nmp-native-runtime` exports `NmpAppBuilder`, a typestate-guarded native runtime
composition root.
The typestate enforces at compile time that:

1. A storage choice (`.in_memory()` or `.storage_path(p)`) is made before `start()`.
2. A projection-consumption decision is made before `start()`.
3. An initial-relay decision is made before `start()`.
4. `start()` is callable exactly once and consumes the builder — no setter is
   reachable post-start.

Add the dependency:

```toml
# Cargo.toml of your app-core crate (or a top-level binary crate)
[dependencies]
nmp-native-runtime = { path = "/path/to/nmp/crates/nmp-native-runtime" }
```

## Minimal read-only shell (~30 lines)

```rust
use std::sync::{Arc, Mutex};
use nmp_native_runtime::{NmpAppBuilder, RunConfig};

// Import your app-core crate — see 19a for how it's structured.
use nostr_feed_core::{NoteRecord, FEED_SNAPSHOT_KEY, project_feed};

fn main() {
    // Wire the app modules into the builder BEFORE starting.
    let mut builder = NmpAppBuilder::new();

    // Seam 2: read output — project the feed into the snapshot.
    let store: Arc<Mutex<Vec<NoteRecord>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let s = Arc::clone(&store);
        builder.register_typed_snapshot_projection(FEED_SNAPSHOT_KEY, move || {
            s.lock().ok().and_then(|g| project_feed(&g))
        });
    }

    // Seam 3: typed read-session helper — owns demand, replay,
    // scoped delivery, and teardown. Observed delivery is internal
    // executor machinery (ADR-0070); shells never call open_observed_projection.
    nostr_feed_core::register_microblog_read_session(&mut builder, Arc::clone(&store));

    // Commit storage, projection, and relay decisions, then start the kernel.
    // Omitting any gate is a COMPILE ERROR (V-94 / ADR-0070 / #1493).
    let app = builder
        .in_memory()
        .declare_consumed_projections([FEED_SNAPSHOT_KEY])
        .without_initial_relays()
        .start(RunConfig::default());

    // The kernel is now running: relay manager started, actor thread live.
    // Consume pushed UpdateFrame bytes through the runtime sink/binding.

    // Shut down cleanly.
    app.stop();
}
```

For production use replace `.in_memory()` with `.storage_path("/path/to/lmdb")`.

## Read-only apps: the empty `Action` enum

Read-only apps (no publishing) still must satisfy codegen's expectation of an
`ActionModule`. Provide an uninhabited action enum — it can never be constructed,
so the impls are never reached:

```rust
// In your app-core lib.rs:
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub enum Action {}  // no variants — unreachable by construction

pub struct MyActionModule;

impl nmp_core::substrate::ActionModule for MyActionModule {
    const NAMESPACE: &'static str = "myapp.action";
    type Action = Action;

    fn start(_: &mut nmp_core::substrate::ActionContext, _: Self::Action)
        -> Result<(), nmp_core::substrate::ActionRejection> { Ok(()) }

    fn execute(
        &self,
        _: &nmp_core::substrate::ActionContext,
        _: Self::Action,
        _: &str,
        _: &dyn Fn(nmp_core::ActorCommand),
    ) -> Result<(), String> { Ok(()) }
}
```

One sentence: "Read-only app? Declare `pub enum Action {}` and an `ActionModule`
that implements both required methods with trivial bodies. The enum has no variants
so neither body is ever invoked."

## Adding publishing (write path)

Bridge-level dispatch for actions such as `PostNote` builds a typed
`DispatchEnvelope` with a host builder and calls the runtime/binding byte
doorway after `start`. App-facing APIs should expose typed intents, not
namespace/body transport helpers:

```rust
let envelope: Vec<u8> = my_app_core::actions::post_note_envelope(
    correlation_id,
    "Hello Nostr",
);

let result = app.dispatch_action_bytes(envelope);
```
// {"correlation_id":"..."} = accepted; {"error":"..."} = rejected.
```

## Generating a local keypair

Before publishing you need a signer. To generate a fresh local key:

```rust
let account = app.create_new_account(CreateAccountOptions::default());
```

The account is created and activated synchronously. The kernel's signer slot is
filled; subsequent `PostNote` dispatches will use this key.

## Standard NIP Wiring

For a full Nostr social app scaffolded by `nmp init`, call your app-core
composition root before `start`. That root installs explicit substrate,
protocol, app, publish/signing, and capability features by name, then wires
app-specific seams. The starter sequence begins with `nmp_substrate::install`,
then calls the owner-crate installers the app actually needs, such as
`nmp_nip50::register_search_scopes`, `nmp_nip02::register_follow_actions`,
`nmp_nip17::register_runtime`, and `nmp_nip23::register_longform_projection`:

```rust
let mut builder = NmpAppBuilder::new();
my_app_core::register(&mut builder);
let app = builder.in_memory().start(RunConfig::default());
```

Compatibility/default presets are deleted. Shells should not compose NMP separately; doing so
creates a second composition path and risks duplicate
registration.

## Lifecycle summary

```
NmpAppBuilder::new()
  │  register_snapshot_projection(...)         ┐ wire before
  │  register_typed_snapshot_projection(...)   │ start — all states
  │  register_microblog_read_session(...)      │ typed helper: demand/replay/scoped-delivery
  │  register_action(M)                  ┘ accept them
  │
  ├─ .in_memory()  or  .storage_path(p)
  │     ↓ NmpAppBuilder<StorageSet>
  └─ .start(RunConfig::default())
        ↓ *mut NmpApp  (kernel running, relays connecting)
        │
        ├─ create_new_account(...)              generate key
        ├─ dispatch_action_bytes(...)           publish
        └─ stop()                               shutdown
```

See also: [19a — scaffold](19a-walkthrough-microblog.md) · [19b — wire & run](19b-walkthrough-microblog.md) · [15 — codegen and FFI](15-codegen-and-ffi.md) · [26 — FAQ / troubleshooting](26-faq-troubleshooting.md)
