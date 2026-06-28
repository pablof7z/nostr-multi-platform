# High-Level Architecture Overview

> **Status:** Proposed design capture for issues #2313 and #2316. This is not a
> shipped API contract. It records the desired shape so an ADR can settle final
> naming, migration order, compatibility scope, and implementation details before
> code changes.

## Authority And Retirement

This packet is a local design workspace for the current architecture iteration.
It is not the canonical tactical queue, not a durable replacement for existing
architecture docs, and not mergeable as a parallel authority in this form.

Before this work becomes a PR, the surviving decisions must move into the
appropriate durable homes: ADRs, existing architecture/design docs, builder-guide
pages, product specs, and GitHub issues for migration work. Anything that remains
only as an exploration artifact should be deleted or explicitly retired. The
point of this directory is to converge on the right shape before editing the
canonical docs, not to add another source of truth.

This is the high-level entry point for the proposed architecture. It explains how
an NMP app feels from the app developer's perspective, then shows how data flows
through NMP's crates. The other docs go deeper into app assembly, live reads,
writes, and internal migration.

It treats #2316 as the problem statement, not as a settled solution.

The core idea is:

```text
install features
  -> open typed feature/ref sessions
  -> render typed Rust-owned outputs
  -> dispatch typed intents
  -> construct/finalize event drafts
  -> sign through a selected signer
  -> publish through Rust-owned routing and status
```

The framework may stay internally complex where Nostr requires it. The app API
must not require developers to manually compose raw interests, observers, cache
replay, dynamic dependency sources, projection sidecars, snapshot ticks, and
teardown recipes.

The destination is simpler because the public unit becomes a whole feature
session lifecycle. It is not simpler because Nostr routing, replay ordering,
projection delivery, signing, and publish policy disappear.

## Complexity Budget

This proposal does not assume today's internal machinery is automatically right.
Each retained concept has to survive a YAGNI review:

```text
what invariant does it protect?
what simpler design was considered?
what breaks if the simpler design is used?
can two existing mechanisms collapse into one?
can this stay private instead of becoming API?
what test or downstream app proves it is needed?
```

Current infrastructure is evidence, not authority. Names in this sketch are
invariants to prove, not commitments to add new Rust types. If
`ObservedProjection`, `ReducedSource`, projection manifests, generated adapters,
publish context, live counts, or any other mechanism cannot defend its cost
against a simpler design, it should be deleted, collapsed, or kept as
migration-scoped compatibility with an owner and deletion/formalization criteria.

## Developer-Level Model

From an app developer's perspective, an NMP app is built out of five things:

1. A Rust app crate defines the product and installs features.
2. Screens, components, widgets, and app services open typed sessions for the
   state they need.
3. Rust emits typed outputs; generated adapters make those outputs pleasant to
   render in Swift, Kotlin, TypeScript, TUI, or another shell.
4. User actions become typed intents or generated builder calls.
5. Native/web shells render UI and execute capabilities. Rust decides what those
   capability results mean.

The app developer should think in terms of product features and typed sessions:

```text
install NIP-29 groups
install profile refs
install app-owned playback

open RoomChat(group)
open NostrAvatar(pubkey)
open PodcastPlayback(app_lifetime)

render room messages
render profile row
render playback state

dispatch SendGroupMessage(...)
dispatch TogglePlayback(...)
```

The app developer should not manually wire:

```text
raw interest + observer + replay + projection sidecar + snapshot tick + teardown
```

That wiring exists, but it is feature/session machinery.

## User-Visible Data Flow

For a user action that reads data:

```text
screen appears
  -> shell opens a typed session
  -> Rust decides the source and route policy
  -> NMP replays cached/store data
  -> NMP subscribes to live relays if needed
  -> Rust reduces events into typed output
  -> shell renders output
  -> screen disappears
  -> shell closes the handle
  -> Rust tears down unowned demand
```

For a user action that writes data:

```text
user taps reply/react/publish
  -> shell dispatches a typed intent
  -> Rust constructs and finalizes the draft
  -> Rust selects the signer
  -> native/web executes the signer capability if needed
  -> Rust validates the signed event
  -> Rust stores it when read-your-writes is allowed
  -> Rust plans publish relays
  -> Rust publishes and records status
  -> shell renders status from typed output
```

The visible product behavior is still simple: open state, render state, dispatch
intent, render updated state. The complex parts are kept behind Rust-owned
feature and runtime seams.

## Crate-Level Flow

The exact crate names can change, but the dependency direction should not:

```text
apps/<app> Rust crate
  -> installs app features and reusable NMP features
  -> depends on NMP crates

crates/nmp-defaults and protocol crates
  -> provide reusable feature bundles, builders, descriptors, parsers, policy
  -> depend on nmp-core seams

crates/nmp-core
  -> owns the actor, state transitions, session lifecycle, projection execution,
     capability requests, signer continuation, publish engine, and typed updates

store/planner/network crates
  -> provide focused infrastructure used by nmp-core and feature crates

runtime/FFI/codegen crates
  -> adapt typed actions, outputs, and capabilities to host platforms

native/web/TUI shells
  -> render outputs and execute raw capabilities
```

`nmp-core` must not import app domains. App crates and protocol crates contribute
typed behavior through seams. Runtime crates adapt the core to host platforms;
they do not decide protocol or product policy.

### Read Path Through Crates

```text
app screen/component
  -> generated/runtime open-session call
  -> app or protocol feature descriptor
  -> nmp-core session lifecycle
  -> nmp-planner / routing policy
  -> nmp-network relay IO when live acquisition is needed
  -> nmp-store / indexed event storage for replay and ingest
  -> nmp-core ObservedProjection and reducers
  -> typed UpdateFrame/output manifest
  -> nmp-ffi, nmp-native-runtime, or nmp-browser-runtime adapter
  -> shell render cache
  -> UI
```

The key point: the shell asks for `RoomChat`, `ProfileRef`, `EventEmbed`, or
`HomeFeed`. It does not ask for naked relay filters and then separately decide
which projections to refresh.

### Write Path Through Crates

```text
user intent in shell
  -> generated typed action / DispatchEnvelope
  -> nmp-ffi or browser/native runtime doorway
  -> nmp-core ActionModule
  -> owning protocol/app feature builder
  -> unsigned-event finalization + routing/privacy context
  -> signer interface and capability bridge
  -> nmp-core publish policy and publish engine
  -> nmp-planner / route policy
  -> nmp-network relay publish
  -> nmp-store local ingest when allowed
  -> typed publish/action status output
  -> shell render
```

The key point: construction, signing, and publishing are separable phases, but
they remain one Rust-owned flow. A native shell can execute a signer or OS
capability, but it does not infer tags, choose relays, retry policy, or publish
state.

### Capability Path Through Crates

```text
Rust feature needs an external effect
  -> nmp-core emits typed capability request
  -> runtime/FFI adapter delivers it to the shell
  -> shell executes OS/API capability
  -> shell reports raw result
  -> nmp-core reducer decides next state
  -> typed output updates UI
```

This is how playback, camera, share extensions, HTTP fetches, Keychain, NIP-55,
NIP-46, Blossom upload, STT, or local AI can fit without moving product policy
into native code.

## Documents

- [App Model](01-app-model.md) explains how an app is assembled, what feature
  bundles provide, and where app-specific Rust domains belong.
- [Live Queries](02-live-queries.md) explains how screens subscribe to data,
  including `ObservedProjection`, `ReducedSource`, component refs, and
  projection delivery.
- [Write Flow](03-write-flow.md) explains the split between event construction,
  event finalization, signing, and publishing.
- [Internal Machinery](04-internal-machinery.md) explains what NMP does under
  the hood and the migration milestones needed to delete the old recipes.

## North Star

An NMP app should be understandable from a small set of concepts:

- A Rust composition root installs explicit feature bundles.
- Screens, components, widgets, and app services open typed sessions for the
  data they render or keep resident.
- Shells render typed outputs produced by Rust and hold only projection caches
  generated for rendering.
- Event construction is composable, protocol-aware, and app-crate extensible.
- Signing is explicit enough to choose a signer, but Rust-owned enough to keep
  native backends interchangeable.
- Publishing applies route policy, protocol pins, delivery, retry, and status in
  Rust.
- Native and web shells render UI and execute capabilities. They do not own
  protocol correctness, durable state, relay planning, or product logic.

## Terms Used Here

The names are deliberately provisional:

- `FeatureSession` or `LiveQuery` means a typed descriptor and handle for the
  live lifecycle a screen, component, widget, or app service opens.
- `ObservedProjection` means the internal safe pattern for replaying cached
  events into a scoped projection before accepting future live events.
- `ReducedSource` means the internal pattern for dynamic query inputs derived
  from other events or account state.
- `EventDraft` means the invariant that unsigned event bytes may still be
  finalized before signing. It is not necessarily a new public type.
- `PublishContext` means the invariant that route, privacy, and protocol policy
  travel with a draft or signed event. It is not necessarily a new type.
- `ReactiveCount` means the invariant for live counts derived from a source and
  filter. A dedicated primitive is justified only if typed projections are not
  enough.

The ADR can rename any of these. The shape is the important part.

## What This Must Fix

This design addresses the concerns behind #2313 and #2316 only if the final
implementation satisfies these constraints:

- `open_interest` stops being taught as the app read model. It may remain only
  in named substrate, protocol-internal, diagnostic, test, or migration scopes.
- `register_defaults()` stops being the mental model for real products. It may
  remain as a named preset for examples, tests, and simple apps.
- Projection tiers stay internal. The app sees typed outputs and handles, not
  `SnapshotRegistry` categories or sidecar rituals.
- Dynamic sources are first-class. Follow lists, group members, visible thread
  roots, embeds, and source fallbacks are Rust-owned descriptors.
- Writes preserve three separable phases: construction/finalization, signing,
  and publishing. They still run through one Rust-owned action/publish path.
- App crates can define product sessions and builders without moving podcast,
  highlighter, playback, capture, queue, or RSS behavior into NMP crates.
