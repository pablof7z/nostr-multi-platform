# 19b — Walkthrough: build a microblog app (wire & run)

**Status: SHIPS · audience: builders.** Part 2 of 2. Continues
[19a](19a-walkthrough-microblog.md) (scaffold). This part runs codegen, wires
the publish path through a real signer, runs on the iOS simulator, and gives
the "what ships today vs tomorrow" milestone matrix.

## Build / run cheatsheet

### 1. Build the app-core crate

```sh
cargo build -p microblog-core
cargo test  -p microblog-core      # exercises register(), ActionModule, observer
```

### 2. Regenerate the per-app FFI crate

Two ways to run codegen. The `nmp` CLI (`crates/nmp-cli`, the user-facing
binary) and the `nmp-codegen` crate binary both support `gen modules`. Inside
the monorepo, use `cargo run -p nmp-codegen` directly:

```sh
cargo run -p nmp-codegen -- gen modules --manifest apps/microblog/nmp.toml
# verify nothing drifted (CI uses this):
cargo run -p nmp-codegen -- gen modules --manifest apps/microblog/nmp.toml --check
```

Or, with the CLI installed (`cargo install --path crates/nmp-cli`):

```sh
nmp gen modules --manifest apps/microblog/nmp.toml
```

> **Honest framing.** This is exactly the command that regenerates the
> existing fixture (`apps/fixture/`). `nmp init` exists today but creates a
> **standalone** project workspace — a separate repo that depends on NMP as a
> path dependency. When adding an app to the NMP monorepo (as this walkthrough
> does), you hand-create `apps/microblog/nmp.toml` and `crates/microblog-core/`
> (as in [19a](19a-walkthrough-microblog.md)), then run `gen modules` to produce
> the FFI crate. The `--out` directory defaults to
> `apps/{name}/{app_crate_name}` per `nmp-codegen/src/main.rs:57-63`.

### 3. Build the FFI library + run on the iOS simulator

The reference shell is **Chirp** (`ios/Chirp/`, the active live iOS app).
It links the Rust static lib and decodes the snapshot via
`ios/Chirp/Chirp/Bridge/KernelBridge.swift`. You point a shell at your FFI
crate's static lib; you do not write a new Swift app from scratch for this
walkthrough.

```sh
# 1. build the Rust staticlib for the sim target
cargo build -p nmp-app-microblog --target aarch64-apple-ios-sim --release
# 2. generate the Xcode project (Chirp uses xcodegen: ios/Chirp/project.yml)
cd ios/Chirp && xcodegen generate
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
  → nmp_app_dispatch_action(NAMESPACE, json)
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
| Legacy raw C FFI (JSON-over-string snapshot) | ✅ today | FlatBuffers migration in progress (F-10); UniFFI binding/lifecycle bridge = **M14, PLANNED** |
| `nmp init` (Rust workspace scaffold) | ✅ ships | Creates a Rust workspace only; full multi-platform starter is M16. This walkthrough hand-scaffolds inside the monorepo. |
| iOS shell (Chirp, active) | ✅ DONE | Additional app shells deferred until Chirp is complete |

The publish substrate, the local signer, multi-account, event observer, and
snapshot projection all ship today. What is *not* shipped: the typed UniFFI
bridge (M14) and a one-command multi-platform scaffolder (M16). The example
above is hand-assembled — that is expected and honest, not a defect.

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
- **Expecting UniFFI typed payload delivery today.** UniFFI is the planned
  binding/lifecycle bridge (M14); it is not the hot update payload format.
  Code that imports a typed UniFFI `AppUpdate` will not compile against master.

See also: [02 — Mental model — kernel + extension seams](02-mental-model.md) ·
[05a — Kernel substrate — traits + seams](05a-substrate-traits.md) ·
[12 — Publishing + the publish engine](12-publish-and-ledger.md) ·
[15 — Codegen — `nmp gen modules` + per-app FFI crate](15-codegen-and-ffi.md) ·
[17 — iOS shell — SwiftUI consumes the kernel](17-ios-shell.md) ·
[19a — Walkthrough: build a microblog app (scaffold)](19a-walkthrough-microblog.md) ·
[20 — Adding a new protocol module](20-new-protocol-module.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
