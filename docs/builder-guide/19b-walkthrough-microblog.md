# 19b — Walkthrough: build a microblog app (wire & run)

**Status: SHIPS · audience: builders.** Part 2 of 2. Continues
[19a](19a-walkthrough-microblog.md) (scaffold). This part runs codegen, wires
the publish path through a real signer, runs on the iOS simulator, and gives
the "what publishes today vs tomorrow" milestone matrix.

## Build / run cheatsheet

### 1. Build the app-core crate

```sh
cargo build -p microblog-core
cargo test  -p microblog-core      # exercises the module_descriptors() registry
```

### 2. Regenerate the per-app FFI crate

The codegen binary is `nmp` (defined in
[`crates/nmp-codegen/src/main.rs:16`](../../crates/nmp-codegen/src/main.rs)).
It reads `nmp.toml` and writes `apps/{name}/{app_crate_name}/src/*`:

```sh
cargo run -p nmp-codegen -- gen modules --manifest apps/microblog/nmp.toml
# verify nothing drifted (CI uses this):
cargo run -p nmp-codegen -- gen modules --manifest apps/microblog/nmp.toml --check
```

> **Honest framing.** This is exactly the command that *regenerates the
> existing fixture* ([`apps/fixture/`](../../apps/fixture)). There is no
> `nmp init` that scaffolds a brand-new app for you yet — that is **M16,
> PLANNED**. Today you hand-create `apps/microblog/nmp.toml` and
> `crates/microblog-core/` (as in [19a](19a-walkthrough-microblog.md)), then
> run `gen modules` to produce the FFI crate. The `--out` directory defaults
> to `apps/{name}/{app_crate_name}` per
> [`main.rs:45-51`](../../crates/nmp-codegen/src/main.rs).

### 3. Build the FFI library + run on the iOS simulator

The reference shell is **NmpStress** (`ios/NmpStress/`, the only live
1,375-LOC iOS app). It links the Rust static lib and decodes the JSON
snapshot via
[`ios/NmpStress/NmpStress/KernelBridge.swift:74-138`](../../ios/NmpStress/NmpStress/KernelBridge.swift).
You point a shell at your FFI crate's static lib; you do not write a new
Swift app from scratch for this walkthrough.

```sh
# 1. build the Rust cdylib/staticlib for the sim target
cargo build -p nmp-app-microblog --target aarch64-apple-ios-sim --release
# 2. generate the Xcode project (NmpStress uses xcodegen: ios/NmpStress/project.yml)
cd ios/NmpStress && xcodegen generate
# 3. build + run on a booted simulator (see section 17 for the bridge details)
```

The Swift side never changes shape: it decodes the same 16-field JSON
snapshot regardless of which app-core is linked (see the field reference in
[26](26-faq-troubleshooting.md)).

## Wiring the publish path

The post action in [19a](19a-walkthrough-microblog.md) ends at
`NoteStep::BuildAndPublish`. The actual publish is the kernel's job, not the
app's. The shipped surface is
[`crates/nmp-core/src/publish/action.rs:40-50`](../../crates/nmp-core/src/publish/action.rs):

```rust
pub enum PublishAction {
    Publish { handle: PublishHandle, event: SignedEvent, target: PublishTarget },
    Cancel  { handle: PublishHandle },
}
pub enum PublishTarget { Auto, Explicit { relays: Vec<RelayUrl> } }
```

`PublishTarget::Auto` defers to the outbox resolver (D3 — NIP-65 routing is
automatic; `Explicit` is the named opt-out). **The app never lists relays.**

The signer is [`LocalKeySigner`](../../crates/nmp-signers/src/signers/local.rs).
Construct it from an `nsec` or hex secret; it derives and caches the pubkey
and signs via the `nostr` crate:

```rust
// crates/nmp-signers/src/signers/local.rs:64,57
let signer = LocalKeySigner::from_nsec("nsec1...")?;       // or:
let signer = LocalKeySigner::from_secret_hex("<64-hex>")?;
// signer.sign(unsigned) -> SignerOp::Ready(Ok(SignedEvent { id, sig, .. }))
//   local.rs:185-187 / 138-173
```

Flow: app `Action::PostNote` → kernel builds an `UnsignedEvent` (kind:1,
content = `text`) → active `IdentityModule` / `LocalKeySigner` signs **once**
(never re-signed on retry — `action.rs:34-39`) → `PublishAction::Publish {
event, target: Auto }` → publish engine fans out per NIP-65 and retries
transient failures (policy is the kernel's, not native's — D7). The app's
`reduce` returns `ActionOutput::Queued`; the per-relay outcome surfaces in
the snapshot, not as a thrown error (D6).

> **Doctrine recap for this flow.** D4: the engine owns per-(event,relay)
> state. D3: `Auto` routing is automatic. D6: no `Result<E>` crosses FFI —
> failures become snapshot rows. D7: retry policy lives in the kernel. The
> app contributes a `text` string and reads a bounded snapshot. That is the
> whole contract.

## What publishes today vs tomorrow — milestone matrix

| Capability | Ships today | Planned |
|---|---|---|
| `PublishAction` substrate + retry queue | ✅ M7 (DONE) | — |
| `LocalKeySigner` (nsec / hex / ncryptsec) | ✅ M6 (DONE) | — |
| Multi-account switch (`AccountManager`) | ✅ M8 (DONE) | — |
| Outbox auto-routing in the planner | ✅ M2 (DONE) | wiring the planner into the *actor's REQ path* is the gap tracked in [27](27-discrepancies.md); the kernel demo still uses constant relays |
| Raw C FFI (JSON-over-string snapshot) | ✅ today | UniFFI typed bridge = **M14, PLANNED** |
| `nmp init` starter CLI | ❌ not built | **M16, PLANNED** — example is hand-scaffolded |
| iOS shell (NmpStress, live) | ✅ M1/M10.5 (DONE) | NmpPodcast/NmpHighlighter are Step-0 scaffolds, not kernel-complete ports |

The publish substrate, the local signer, and multi-account all ship today.
What is *not* shipped: the typed UniFFI bridge (M14) and a one-command app
scaffolder (M16). The example above is therefore hand-assembled — that is
expected and honest, not a defect.

## Anti-patterns (wire & run phase)

- **Building, signing, or publishing the event in the app.** The app emits
  `Action::PostNote { text }`. The kernel builds the `UnsignedEvent`, the
  signer signs once, the engine publishes and retries. App-side
  build-sign-publish duplicates kernel state and breaks D4/D7.
- **Passing relay URLs from app code.** There is no relay parameter on the
  post action. `PublishTarget::Auto` routes via NIP-65 (D3). Hardcoding
  relays in the app is the named opt-out, not the default — and almost
  always a mistake in a microblog.
- **Manual REQ in app code to "refresh the feed."** The `FeedViewModule`
  snapshot updates reactively. A manual REQ scan parallel to the kernel is
  a D2/D4 violation; the feed is a projection, not something you poll.
- **Per-platform SwiftData/Room cache parallel to `AppState`.** The JSON
  snapshot is the single source of truth across FFI. A native cache shadowing
  it drifts and violates D5.
- **Expecting `nmp init` or UniFFI today.** Both are PLANNED milestones.
  Code that imports a typed UniFFI `AppUpdate` will not compile against
  master.

See also: [02 — Mental model — kernel + 5 trait families](02-mental-model.md) ·
[05 — Kernel substrate — the 5 trait families](05-substrate-traits.md) ·
[12 — Publishing + the publish engine](12-publish-and-ledger.md) ·
[15 — Codegen — `nmp gen modules` + per-app FFI crate](15-codegen-and-ffi.md) ·
[17 — iOS shell — SwiftUI consumes the kernel](17-ios-shell.md) ·
[19a — Walkthrough: build a microblog app (scaffold)](19a-walkthrough-microblog.md) ·
[20 — Adding a new protocol module](20-new-protocol-module.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
