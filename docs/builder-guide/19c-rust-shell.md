# 19c — Rust-native shell: bootstrapping the kernel from Rust

**Status: SHIPS** · Audience: builders · Dependency: [19a](19a-walkthrough-microblog.md), [19b](19b-walkthrough-microblog.md)

This section answers the question the walkthrough leaves open: *how do I actually
start the NMP kernel from a Rust binary?* Use this when your host is a Rust TUI,
a headless test harness, a CLI tool, or any Rust process — not an iOS/Android
native shell.

## The entry point: `NmpAppBuilder`

`nmp-app-template` ships `NmpAppBuilder`, a typestate-guarded composition root.
The typestate enforces at compile time that:

1. A storage choice (`.in_memory()` or `.storage_path(p)`) is made before `start()`.
2. `start()` is callable exactly once and consumes the builder — no setter is
   reachable post-start.

Add the dependency:

```toml
# Cargo.toml of your app-core crate (or a top-level binary crate)
[dependencies]
nmp-app-template = { path = "/path/to/nmp/crates/nmp-app-template" }
nmp-ffi          = { path = "/path/to/nmp/crates/nmp-ffi" }
```

## Minimal read-only shell (~30 lines)

```rust
use std::sync::{Arc, Mutex};
use nmp_app_template::{NmpAppBuilder, RunConfig};
use nmp_ffi::{nmp_app_free, nmp_app_stop};

// Import your app-core crate — see 19a for how it's structured.
use nostr_feed_core::{FeedObserver, NoteRecord, FEED_SNAPSHOT_KEY, project_feed};

fn main() {
    // Wire the app modules into the builder BEFORE starting.
    let mut builder = NmpAppBuilder::new();

    // Seam 2: read output — project the feed into the snapshot.
    let store: Arc<Mutex<Vec<NoteRecord>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let s = Arc::clone(&store);
        builder.register_snapshot_projection(FEED_SNAPSHOT_KEY, move || {
            s.lock().map(|g| project_feed(&g)).unwrap_or(serde_json::Value::Null)
        });
    }

    // Seam 3: event-driven view — observer populates the store.
    let _ = builder.register_event_observer(Arc::new(FeedObserver::new(Arc::clone(&store))));

    // Commit the storage choice and start the kernel.
    // .in_memory()  →  NmpAppBuilder<StorageSet>  →  .start()  →  *mut NmpApp
    // Omitting .in_memory()/.storage_path() is a COMPILE ERROR (V-94).
    let app = builder.in_memory().start(RunConfig::default());

    // The kernel is now running: relay manager started, actor thread live.
    // Read the snapshot whenever you want:
    // let snap = unsafe { read_snapshot(app) };  // see §15 / nmp_app_get_snapshot

    // Shut down cleanly.
    nmp_app_stop(app);
    nmp_app_free(app);
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

pub struct AppActionModule;

impl nmp_core::substrate::ActionModule for AppActionModule {
    const NAMESPACE: &'static str = "myapp.action";
    type Action = Action;

    fn start(_: &mut nmp_core::substrate::ActionContext, _: Self::Action)
        -> Result<(), nmp_core::substrate::ActionRejection> { Ok(()) }

    fn execute(_: Self::Action, _: &str,
        _: &dyn Fn(nmp_core::ActorCommand)) -> Result<(), String> { Ok(()) }
}
```

One sentence: "Read-only app? Declare `pub enum Action {}` and an `ActionModule`
that implements both required methods with trivial bodies. The enum has no variants
so neither body is ever invoked."

## Adding publishing (write path)

To dispatch actions (e.g. `PostNote`) call `nmp_app_dispatch_action` after `start`:

```rust
use std::ffi::CString;
use nmp_ffi::nmp_app_dispatch_action;

// Serialize the action value to JSON, then dispatch.
let action_json = serde_json::to_string(&my_app_core::Action::PostNote {
    text: "Hello Nostr".into(),
}).unwrap();
let ns   = CString::new(my_app_core::PostNoteActionModule::NAMESPACE).unwrap();
let body = CString::new(action_json).unwrap();

// SAFETY: app is valid; ns/body are NUL-free C strings.
let result_ptr = unsafe { nmp_app_dispatch_action(app, ns.as_ptr(), body.as_ptr()) };

// result_ptr is a heap-allocated JSON string; free it after reading.
let result = unsafe { std::ffi::CStr::from_ptr(result_ptr) }
    .to_string_lossy().into_owned();
nmp_ffi::nmp_app_free_string(result_ptr);

// {"correlation_id":"..."} = accepted; {"error":"..."} = rejected.
```

## Generating a local keypair

Before publishing you need a signer. To generate a fresh local key:

```rust
use std::ffi::CString;
use nmp_ffi::nmp_app_create_new_account;

// All three optional args (profile JSON, relays JSON, MLS flag) can be null/false.
let null_ptr: *const std::ffi::c_char = std::ptr::null();
// SAFETY: app is valid; null pointers are accepted by the C-ABI (defaults apply).
unsafe { nmp_app_create_new_account(app, null_ptr, null_ptr, false) };
```

The account is created and activated synchronously. The kernel's signer slot is
filled; subsequent `PostNote` dispatches will use this key.

## Standard NIP wiring (`register_defaults`)

For a full Nostr social app (NIP-02 follows, NIP-17 DMs, NIP-57 zaps, NIP-65
relay lists) call `register_defaults` before `start`:

```rust
let mut builder = NmpAppBuilder::new();
nmp_app_template::register_defaults(&mut builder);
// … then register your own projections/observers …
let app = builder.in_memory().start(RunConfig::default());
```

`register_defaults` is the one function that wires the standard NMP protocol
suite. New apps should call it unless they need a stripped-down kernel.

## Lifecycle summary

```
NmpAppBuilder::new()
  │  register_snapshot_projection(...)   ┐ wire before
  │  register_event_observer(...)        │ start — both states
  │  register_action::<M>()             ┘ accept them
  │
  ├─ .in_memory()  or  .storage_path(p)
  │     ↓ NmpAppBuilder<StorageSet>
  └─ .start(RunConfig::default())
        ↓ *mut NmpApp  (kernel running, relays connecting)
        │
        ├─ nmp_app_create_new_account(...)   generate key
        ├─ nmp_app_dispatch_action(...)      publish
        └─ nmp_app_stop(app) + nmp_app_free(app)  shutdown
```

See also: [19a — scaffold](19a-walkthrough-microblog.md) · [19b — wire & run](19b-walkthrough-microblog.md) · [15 — codegen and FFI](15-codegen-and-ffi.md) · [26 — FAQ / troubleshooting](26-faq-troubleshooting.md)
