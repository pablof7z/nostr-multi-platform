# 19b — Walkthrough: build a microblog app (wire & run)

**Status: SHIPS · audience: builders.** Part 2 of 2. Continues
[19a](19a-walkthrough-microblog.md) (scaffold). This part creates the thin
binding adapter, wires the publish path through a real signer, runs from a
native shell, and gives the clean-break capability matrix.

There is **no codegen step**. Composition is explicit Rust in the app-core crate,
not a generated per-app FFI crate (ADR-0069). The adapter you create here is a
handful of lines of glue.

## Build / run cheatsheet

### 1. Build the app-core crate

```sh
cargo build -p microblog-core
cargo test  -p microblog-core      # exercises register(), ActionModule, session helper
```

### 2. Create the thin binding adapter

`apps/microblog/nmp-app-microblog/Cargo.toml`:

```toml
[package]
name = "nmp-app-microblog"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
nmp-core = { path = "../../../crates/nmp-core" }
nmp-native-runtime = { path = "../../../crates/nmp-native-runtime" }
microblog-core = { path = "../../../crates/microblog-core" }
```

`apps/microblog/nmp-app-microblog/src/lib.rs`:

```rust
//! Thin binding adapter. No app logic here; everything lives in microblog-core.
use nmp_core::substrate::AppHost;

/// Configure the microblog app. `microblog_core::register` is the composition root:
/// it installs explicit substrate/protocol pieces, then the microblog seams.
/// Generated native/browser bindings call this before the runtime starts.
pub fn configure_microblog(app: &mut impl AppHost) {
    microblog_core::register(app);
}
```

If you are building a headless example rather than an iOS shell, the same
composition fits in `examples/shell.rs` using the native runtime builder:

```rust
use nmp_native_runtime::{NmpAppBuilder, RunConfig};

fn main() {
    let mut builder = NmpAppBuilder::new();
    microblog_core::register(&mut builder);
    let app = builder
        .storage_path("/tmp/microblog-data")
        .declare_consumed_projections(["microblog.items"])
        .with_relays([("wss://relay.example", "both")])
        .start(RunConfig::default());
    // Native UniFFI and browser wasm-bindgen adapters drive the same runtime.
}
```

### 3. Build the adapter + run from a native shell

The reference shell imports generated UniFFI bindings, starts the configured
runtime, dispatches typed action bytes, and decodes the pushed FlatBuffers
`UpdateFrame`. For this walkthrough, point the binding adapter at
`configure_microblog` before start.

```swift
let app = NmpAppHandle()
app.configureMicroblog()
app.setUpdateSink(updateSink)
app.start()
```

```sh
# 1. build the Rust adapter for the target
cargo build -p nmp-app-microblog --release
# 2. generate/import the UniFFI bindings for the native shell
# 3. build + run the shell (see section 17 for the bridge details)
```

The Swift side reads `typed_projections["microblog.items"]` from the pushed
`SnapshotFrame` alongside the built-in fields (see
[17 — iOS shell](17-ios-shell.md) §Reading a typed projection in `apply()`).

## How publish flows

The `PostNote` action in [19a](19a-walkthrough-microblog.md) calls
`send(ActorCommand::PublishNote { content: text, … })` inside `execute`.
The actual signing and routing is entirely the kernel's job:

```
typed app intent: postNote(text)
  → UniFFI dispatchActionBytes(DispatchEnvelope)
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

`is_async_completing() = true` means the byte action doorway returns immediately with
`{ "correlation_id": "…" }`. The terminal outcome (`Publishing → Accepted /
Failed`) arrives later through the snapshot's `projections["action_stages"]`
map keyed by that id.

## What ships today vs tomorrow — milestone matrix

| Capability | Ships today | Planned |
|---|---|---|
| `ActorCommand::PublishNote` (kind:1 sign + outbox publish) | ✅ DONE | — |
| arbitrary-kind publish command | ✅ DONE (low-level escape; not starter app guidance) | — |
| `LocalKeySigner` (nsec / hex / ncryptsec) | ✅ M6 (DONE) | — |
| NIP-46 bunker signer | ✅ M6 (DONE) | — |
| Multi-account switch | ✅ M8 (DONE) | — |
| Outbox auto-routing (NIP-65) | ✅ T105 (DONE) | — |
| typed read-session helpers | ✅ DONE | — |
| typed output transport | ✅ DONE | — |
| UniFFI native lifecycle/action binding + FlatBuffers update frames | ✅ public native path | — |
| wasm-bindgen browser action/update binding | ✅ public browser path | — |
| `nmp init` (thin Rust shell scaffold) | ✅ ships | Creates a `<name>-core` crate + `examples/shell.rs`; full multi-platform starter is M16. |
| Native shell proof | ✅ DONE | Gallery is the in-tree shell proof; Chirp is an external consumer |

The publish substrate, the local signer, multi-account, typed read sessions, and
typed output transport all ship today. What is *not* shipped is a one-command
multi-platform scaffolder (M16). The example above is hand-assembled — that is
expected and honest, not a defect.

## Anti-patterns (wire & run phase)

- **Building, signing, or publishing the event in the app.** The app emits
  `Action::PostNote { text }`. The actor fills pubkey, timestamps, signs, and
  publishes. App-side build-sign-publish duplicates kernel state and breaks D4/D7.
- **Passing relay URLs from app code.** There is no relay parameter on the
  post action. `PublishNote` routes via NIP-65 outbox (D3). Hardcoding
  relays is the named opt-out, not the default.
- **Manual REQ in app code to "refresh the feed."** The typed read-session
  helper updates reactively. A manual REQ scan parallel to the kernel is a
  D2/D4 violation; the feed is typed output, not something you poll.
- **Per-platform SwiftData/Room cache parallel to `AppState`.** The decoded
  snapshot is the single source of truth across FFI. A native cache shadowing
  it drifts and violates D5.
- **Expecting generated framework wiring.** The binding adapter is hand-written
  glue that calls the app-core composition root; see
  [15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md).
- **Expecting UniFFI to own hot updates.** UniFFI owns native object/callback
  binding; update payloads use the typed FlatBuffers stream.

See also: [02 — Mental model — kernel + extension seams](02-mental-model.md) ·
[05a — Kernel substrate — traits + seams](05a-substrate-traits.md) ·
[12 — Publishing + the publish engine](12-publish-and-ledger.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[17 — iOS shell — SwiftUI consumes the kernel](17-ios-shell.md) ·
[19a — Walkthrough: build a microblog app (scaffold)](19a-walkthrough-microblog.md) ·
[20 — Adding a new protocol module](20-new-protocol-module.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
