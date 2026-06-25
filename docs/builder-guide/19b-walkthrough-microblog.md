# 19b — Walkthrough: build a microblog app (wire & run)

**Status: SHIPS · audience: builders.** Part 2 of 2. Continues
[19a](19a-walkthrough-microblog.md) (scaffold). This part creates the thin
staticlib shell, wires the publish path through a real signer, runs on the iOS
simulator, and gives the "what ships today vs tomorrow" milestone matrix.

There is **no codegen step**. Composition is a library call
(`nmp_defaults::register_defaults`), not a generated per-app FFI crate
(ADR-0046). The shell you create here is a handful of lines of glue.

## Build / run cheatsheet

### 1. Build the app-core crate

```sh
cargo build -p microblog-core
cargo test  -p microblog-core      # exercises register(), ActionModule, observer
```

### 2. Create the thin staticlib shell

`apps/microblog/nmp-app-microblog/Cargo.toml`:

```toml
[package]
name = "nmp-app-microblog"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "nmp_app_microblog"
crate-type = ["staticlib", "rlib"]

[dependencies]
nmp-core = { path = "../../../crates/nmp-core" }
nmp-ffi = { path = "../../../crates/nmp-ffi" }
nmp-defaults = { path = "../../../crates/nmp-defaults" }
microblog-core = { path = "../../../crates/microblog-core" }
```

`apps/microblog/nmp-app-microblog/src/lib.rs`:

```rust
//! Thin staticlib shell. No app logic here; everything lives in microblog-core.
use nmp_ffi::NmpApp;

/// Register the microblog app. `microblog_core::register` is the composition root:
/// it installs the canonical NMP defaults once, then the microblog seams.
/// The iOS shell calls this after `nmp_app_new()` and before `nmp_app_start()`.
#[no_mangle]
pub extern "C" fn nmp_app_microblog_register(app: *mut NmpApp) {
    if app.is_null() {
        return;
    }
    // SAFETY: caller guarantees `app` is a valid pointer from `nmp_app_new()`.
    // No other reference aliases it here — the exclusive borrow is released
    // before any shared-borrow registration calls below.
    microblog_core::register(unsafe { &mut *app });
}
```

If you are building a headless example rather than an iOS shell, the same
composition fits in `examples/shell.rs` using `NmpAppBuilder`:

```rust
use nmp_defaults::{NmpAppBuilder, RunConfig};

fn main() {
    let mut builder = NmpAppBuilder::new();
    microblog_core::register(&mut builder);
    let app = builder
        .storage_path("/tmp/microblog-data")
        .declare_consumed_projections(["microblog.items"])
        .start(RunConfig::default());
    // Drive the app via `nmp_ffi` symbols (set update callback, dispatch
    // actions, etc.), then `nmp_ffi::nmp_app_free(app)`.
}
```

### 3. Build the static lib + run on the iOS simulator

The reference shell is **Chirp** (`apps/chirp/ios/`, the active live iOS app).
It links the Rust static lib and decodes the snapshot via
`apps/chirp/ios/Chirp/Bridge/KernelBridge.swift`. For this walkthrough, point a
shell at your static lib and call `nmp_app_microblog_register` instead of
`nmp_app_chirp_register`.

```swift
raw = nmp_app_new()
precondition(nmp_signer_broker_init(raw) == 0)
nmp_app_microblog_register(raw)   // your registration symbol
nmp_app_set_update_callback(raw, ..., nmpUpdateCallback)
nmp_app_start(raw, 0, 80, 4)
```

```sh
# 1. build the Rust staticlib for the sim target
cargo build -p nmp-app-microblog --target aarch64-apple-ios-sim --release
# 2. generate the Xcode project (Chirp uses xcodegen: apps/chirp/ios/project.yml)
cd apps/chirp/ios && xcodegen generate
# 3. build + run on a booted simulator (see section 17 for the bridge details)
```

The Swift side reads `projections["microblog.items"]` from the snapshot's
`projections` map alongside the built-in fields (see
[17 — iOS shell](17-ios-shell.md) §Reading a snapshot projection in `apply()`).

## How publish flows

The `PostNote` action in [19a](19a-walkthrough-microblog.md) calls
`send(ActorCommand::PublishNote { content: text, … })` inside `execute`.
The actual signing and routing is entirely the kernel's job:

```
app dispatch(PostNote { text })
  → nmp_app_dispatch_action("microblog.action", json)
  → ActionModule::start() validates (non-empty text)
  → ActionModule::execute() calls send(ActorCommand::PublishNote { … })
  → actor thread receives PublishNote
  → fills pubkey from active signer, stamps created_at from kernel.now_secs() (D9)
  → signs once (local key: immediate; NIP-46 bunker: async via PendingSign, D8)
  → PublishEngine fans out to author's NIP-65 write relays (D3)
  → per-relay ACK surfaces in projections["action_stages"][correlation_id] (D6)
```

**The app contributes `text`. The kernel decides pubkey, timestamp, relays,
retry policy.** That is the whole write contract.

`is_async_completing() = true` means `dispatch_action` returns immediately with
`{ "correlation_id": "…" }`. The terminal outcome (`Publishing → Accepted /
Failed`) arrives later through the snapshot's `projections["action_stages"]`
map keyed by that id.

## What ships today vs tomorrow — milestone matrix

| Capability | Ships today | Planned |
|---|---|---|
| `ActorCommand::PublishNote` (kind:1 sign + outbox publish) | ✅ DONE | — |
| `ActorCommand::PublishRawEvent` (arbitrary kind) | ✅ DONE | — |
| `LocalKeySigner` (nsec / hex / ncryptsec) | ✅ M6 (DONE) | — |
| NIP-46 bunker signer | ✅ M6 (DONE) | — |
| Multi-account switch | ✅ M8 (DONE) | — |
| Outbox auto-routing (NIP-65) | ✅ T105 (DONE) | — |
| `KernelEventObserver` + `register_event_observer` | ✅ DONE | — |
| `register_snapshot_projection` | ✅ DONE | — |
| Raw C/JNI lifecycle/action FFI + FlatBuffers update frames | ✅ today | UniFFI binding/lifecycle bridge = **M14, PLANNED** |
| `nmp init` (thin Rust shell scaffold) | ✅ ships | Creates a `<name>-core` crate + `examples/shell.rs`; full multi-platform starter is M16. |
| iOS shell (Chirp, active) | ✅ DONE | Additional app shells deferred until Chirp is complete |

The publish substrate, the local signer, multi-account, event observer, and
snapshot projection all ship today. What is *not* shipped: the typed UniFFI
bridge (M14), a one-command multi-platform scaffolder (M16), and the old
`nmp gen modules` generator (deleted by ADR-0046). The example above is hand-
assembled — that is expected and honest, not a defect.

## Anti-patterns (wire & run phase)

- **Building, signing, or publishing the event in the app.** The app emits
  `Action::PostNote { text }`. The actor fills pubkey, timestamps, signs, and
  publishes. App-side build-sign-publish duplicates kernel state and breaks D4/D7.
- **Passing relay URLs from app code.** There is no relay parameter on the
  post action. `PublishNote` routes via NIP-65 outbox (D3). Hardcoding
  relays is the named opt-out, not the default.
- **Manual REQ in app code to "refresh the feed."** The feed store updates
  reactively via `on_kernel_event`. A manual REQ scan parallel to the kernel
  is a D2/D4 violation; the feed is a projection, not something you poll.
- **Per-platform SwiftData/Room cache parallel to `AppState`.** The decoded
  snapshot is the single source of truth across FFI. A native cache shadowing
  it drifts and violates D5.
- **Expecting a generated per-app FFI crate today.** `gen modules` is gone.
  The staticlib shell is hand-written glue that calls the app-core composition
  root; see [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md).
- **Expecting UniFFI typed payload delivery today.** UniFFI is the planned
  binding/lifecycle bridge (M14); it is not the hot update payload format.
  Code that imports a typed UniFFI `AppUpdate` will not compile against master.

See also: [02 — Mental model — kernel + extension seams](02-mental-model.md) ·
[05a — Kernel substrate — traits + seams](05a-substrate-traits.md) ·
[12 — Publishing + the publish engine](12-publish-and-ledger.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[17 — iOS shell — SwiftUI consumes the kernel](17-ios-shell.md) ·
[19a — Walkthrough: build a microblog app (scaffold)](19a-walkthrough-microblog.md) ·
[20 — Adding a new protocol module](20-new-protocol-module.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
