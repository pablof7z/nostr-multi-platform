# v1 Runtime And Component Migration Guide

> **Status:** v1 durable guide. This document describes the settled migration
> target, not the history of how the target landed. Tactical release status
> stays in GitHub Issues.

Use this guide when moving a downstream app or example from the pre-v1 runtime,
projection, and component shape to the v1 NMP surface.

## Target Shape

The v1 split is:

| Surface | Owns | Must not own |
|---|---|---|
| `nmp-substrate` | Shared substrate floor: router/mailbox/profile/contacts cache-parser construction, publish resolver, coverage hook, NIP-77 interceptors, blocked-relay wiring, and native NIP-11 hook. | Protocol/product feature bundles, platform runtime handles, C ABI symbols, operator policy, app defaults. |
| `nmp-native-runtime` | Native runtime handle, actor lifecycle, native typestate builder, runtime slots, pre-start configuration, and native Rust APIs. | C ABI conversion or app/product policy. |
| App-owned UniFFI facade + `nmp-uniffi-support` | Public native binding surface over the native runtime: facade-local lifecycle object, callbacks/sinks, typed dispatch bytes, typed read-session helpers, diagnostics, generated Swift/Kotlin bindings, and shared Rust mechanics reused from `nmp-uniffi-support`. | Runtime ownership, composition policy, protocol logic, hot snapshot payload format, or a reusable framework-generated binding namespace. |
| `nmp-browser-runtime` | Browser Worker runtime, wasm-bindgen export, wasm-bindgen ABI glue (`nmp-browser-runtime::wasm`), browser typestate builder, storage/signing/capability provider registration. | UI rendering, TypeScript crypto fallbacks, protocol policy. |
| App-owned delivery glue | Local shell adapters such as Gallery-specific C/JNI helpers when a concrete app still needs them. | Reusable framework API, starter setup guidance, runtime ownership, protocol logic. |

App shells remain thin. They render snapshots, execute platform capabilities,
and hold only ephemeral presentation state. Rust owns protocol behavior, durable
state, routing, signing policy, and projection derivation.

## Native Runtime Split

Before, examples often treated the deleted defaults bundle or the deleted raw C
ABI shell as the native runtime owner:

```rust
use old_defaults::{NmpAppBuilder, RunConfig};

let app = NmpAppBuilder::new()
    .in_memory()
    .start(RunConfig::default());
```

After migration, native runtime construction comes from `nmp-native-runtime` and
must pass the typestate gates:

```rust
use nmp_native_runtime::{NmpAppBuilder, RunConfig};

let app = NmpAppBuilder::new()
    .in_memory()
    .declare_consumed_projections(["refs.profile", "refs.event"])
    .with_relays([("wss://relay.example", "both")])
    .start(RunConfig::default());
```

If the app has no startup relays, make that explicit:

```rust
let app = NmpAppBuilder::new()
    .in_memory()
    .declare_consumed_projections(["refs.profile"])
    .without_initial_relays()
    .start(RunConfig::default());
```

Swift and Kotlin callers should use generated UniFFI bindings over the native
runtime. New native runtime behavior belongs in `nmp-native-runtime`; binding
crates only expose and marshal it.

## Defaults Composition

Before, downstream apps copied template wiring or expected generated per-app
composition:

```rust
// Bad v1 shape: copied framework wiring or a generated composition crate.
pub fn register_everything(app: &mut NmpApp) {
    copy_router_setup(app);
    copy_profile_setup(app);
    app.register_action(MyActionModule);
}
```

After migration, the app core is the composition root. It installs the substrate
floor and selected protocol/app seams by name:

```rust
use nmp_core::substrate::AppHost;

pub fn register(app: &mut impl AppHost) {
    let _substrate_handles =
        nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());
    nmp_nip50::register(app, nmp_nip50::Config::default())?;
    nmp_nip02::register(app, nmp_nip02::Config::default())?;
    nmp_replies::register(app, nmp_replies::Config::default())?;
    nmp_nip17::register(app, nmp_nip17::Config::default())?;
    nmp_nip22::register(app, nmp_nip22::Config::default())?;
    nmp_nip23::register(app, nmp_nip23::Config::default())?;
    my_app_core::register_actions(app);
    my_app_core::register_projections(app);
}
```

### Protocol installers

Old split calls:

```rust
nmp_nip17::register_actions(app);
nmp_nip17::register_runtime(app);
```

New canonical call:

```rust
let _nip17 = nmp_nip17::register(app, nmp_nip17::Config::default())?;
```

Every protocol crate exposes `Config`, `Handles`, and `register`. Public
`register_*` aliases are intentionally absent so a protocol cannot be
half-installed.

App-owned relays, seed follows, signer permissions, and product defaults stay
in the leaf app Rust crate or operator config. They do not move into a shared
defaults bundle. Starter apps must keep the named installer sequence visible;
wholesale defaults presets, compatibility shims, and test-helper bundles are
not current app-facing model.

## Browser Runtime

Before, browser code often looked at TypeScript as the runtime owner:

```ts
// Bad v1 shape: app code treats wasm/TS as a policy owner.
worker.postMessage({
  type: "dispatch_json",
  action_namespace: "nmp.nip17.send",
  body: JSON.stringify(payload),
});
```

After migration, `nmp-browser-runtime` owns the Worker runtime and the browser
builder. App writes use a finished `DispatchEnvelope` byte buffer:

```ts
import { GeneratedActionBuilders } from "@nmp/runtime-web";

const bytes = GeneratedActionBuilders.sendDm(
  correlationId,
  recipientPubkey,
  content,
  null,
);

worker.postMessage({ type: "dispatch_bytes", bytes }, [bytes.buffer]);
```

The Worker routes those bytes through the same typed action doorway as the
native UniFFI dispatch-byte method. There is no wasm-only write vocabulary.

Signer capability is not uniform across browser backends:

- `local_key` sessions satisfy signing and NIP-44 inside Rust for the current
  browser session.
- `nip46` sessions route signing and NIP-44 through the Rust-owned NIP-46
  provider path.
- `nip07` sessions can sign through the extension bridge, but NIP-44 only works
  when the extension exposes `window.nostr.nip44.encrypt` and
  `window.nostr.nip44.decrypt`.

Do not call `window.nostr.nip44` directly from UI code. The detailed browser
private-flow matrix lives in `docs/wasm-surface.md` and the NIP summary lives in
`docs/nips.md`; #2255 tracks the docs correction that established that model.

## Reference Projections

The live content reference shape is:

- `refs.profile`: authoritative keyed profile rows.
- `refs.event`: authoritative keyed event-reference rows.
- `refs.event.envelopes`: derived render envelopes produced from `refs.event`
  by `nmp-content`.

Before:

```swift
// Bad v1 shape: whole-map legacy projection.
let profile = snapshot.projections["resolved_profiles"]?[pubkey]
let embed = snapshot.projections["claimed_event_embeds"]?[eventId]
```

After:

```swift
// Host mirror fed by typed row-delta sidecars.
let profile = keyedRefCache.profileCard(forPubkey: pubkey)
let eventRow = keyedRefCache.eventRef(forKey: primaryId)
let envelope = embedHost.envelope(primaryId: primaryId)
```

`refs.event` is the source of truth. `refs.event.envelopes` is render data. A
shell or component package must not parse raw Nostr event JSON, dispatch on
kinds, or assemble embed envelopes itself.

## Component Host Adoption

The component-host contract is one app root provider over app-owned projection
mirrors and resolvers:

- SwiftUI installs `NmpComponentHost` once near the app or screen root.
- Compose installs `NmpComponentHostProvider(...)` once near the app or screen
  root.
- Web/Solid installs `NmpComponentHostProvider(...)` from
  `@nmp/components-web` once near the app root.

Leaf components render and manage visible claim/release lifecycle only. They do
not import `nmp-ffi`, `nmp-native-runtime`, `nmp-browser-runtime`, kernel
handles, worker handles, or relay/runtime internals.

Before:

```kotlin
// Bad v1 shape: every screen builds its own low-level bridge.
EmbeddedEvent(
    rawEventJson = raw,
    resolve = { kernelHandle.resolveEvent(it) },
)
```

After:

```kotlin
NmpComponentHostProvider(
    profileHost = appProfileHost,
    resolvedEventEmbeds = refsEventEnvelopesMirror,
    eventRefResolver = appEventRefResolver,
) {
    EmbeddedEvent(uri = uri, primaryId = primaryId)
}
```

#2257 owns the mechanical conformance kit: fake/in-memory hosts, component
fixtures, dependency guards, and registry/export checks that prove components
stay on the host/provider path. Do not claim component-host integration is
mechanically guarded until the #2257 checks are present and green.

## Typed Action Dispatch

Before:

```swift
// Bad v1 shape: shell spells namespaces and JSON body by hand.
bridge.dispatch(namespace: "nmp.nip25.react", bodyJson: body)
```

After:

```swift
let bytes = GeneratedActionBuilders.react(
    correlationId: correlationId,
    targetEventId: eventId,
    reaction: "+",
    targetAuthorPubkey: nil
)
bridge.dispatchBytes(bytes)
```

The generated builder owns the action namespace and payload encoding. Native and
web hosts only mint correlation ids, pass typed input into generated builders,
and send the resulting bytes through the dispatch doorway.

## Validation Commands

Run the smallest app-specific tests first, then the gates that prove boundary
behavior:

```bash
cargo test -p <your-app-core>
cargo build -p <your-native-shell-crate>
cargo test -p nmp-testing --test doctrine_lint_smoke -- --test-threads=1
git diff --check
```

If you changed public symbols, dependency paths, generated action builders, or
workspace members, also run:

```bash
cargo build --workspace
cargo run -p nmp-codegen -- gen action-builders --platform ts \
  --out web/packages/runtime-web/src/actionBuilders.generated.ts
cargo test -p nmp-cli --test component_registry_metadata
```

For browser runtime work, add the browser-runtime checks from the current CI
workflow:

```bash
cargo test -p nmp-browser-runtime
cargo build -p nmp-browser-runtime --target wasm32-unknown-unknown --features wasm
```

For component-host migrations, keep local host/provider tests until #2257 lands,
then run the #2257 conformance kit for the affected SwiftUI, Compose, and/or web
component package.

## Downstream App Checklist

- Runtime construction imports `NmpAppBuilder` / `RunConfig` from
  `nmp-native-runtime` for native Rust hosts, or `BrowserAppBuilder` /
  `NmpWasmRuntime` from `nmp-browser-runtime` for browser hosts.
- Swift/Kotlin app shells consume generated UniFFI bindings over the native
  runtime; raw C/JNI symbols are not the starter setup path.
- `nmp-substrate::install(...)` is called for the shared substrate floor; no app
  copies router/mailbox/profile/contacts construction.
- App/operator policy stays in the leaf app Rust crate or config, not in
  shared substrate/protocol crates.
- Native and web writes go through generated action builders and dispatch bytes.
- Shells no longer spell action namespaces as ad hoc strings.
- Profile UI reads `refs.profile`, not `resolved_profiles`.
- Event/embed UI reads authoritative `refs.event` plus derived
  `refs.event.envelopes`, not `claimed_event_embeds`.
- Component packages receive one app-level host/provider and do not import
  runtime, ABI, worker, or kernel handles.
- Browser signer docs distinguish local-key, NIP-46, and NIP-07 NIP-44 support;
  keep `docs/wasm-surface.md` and `docs/nips.md` aligned with that matrix.
- The app has a regenerated binding/decoder/action-builder baseline and the
  validation commands above pass.
