# New Architecture Sketch

> **Status:** Proposed design capture for issues #2313 and #2316. This is not a
> shipped API contract. It records the desired shape so an ADR can settle final
> naming, migration order, compatibility scope, and implementation details before
> code changes.

This sketch describes how an NMP app should feel to build from a developer
perspective, and what NMP should do internally to make that simple surface true.
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

- `FeatureSession` or `LiveQuery` means the app-facing live lifecycle a screen,
  component, widget, or app service opens.
- `ObservedProjection` means the internal safe pattern for replaying cached
  events into a scoped projection before accepting future live events.
- `ReducedSource` means the internal pattern for dynamic query inputs derived
  from other events or account state.
- `EventDraft` means unsigned event bytes plus protocol context that may still
  be mutated before signing.
- `PublishContext` means route, privacy, and protocol policy that travels with a
  draft or signed event until publish is complete.
- `ReactiveCount` means a reusable live count output derived from a source and
  filter, not a hard-coded engagement product.

The ADR can rename any of these. The shape is the important part.

## What This Must Fix

This design addresses the concerns behind #2313 and #2316 only if the final
implementation satisfies these constraints:

- `open_interest` stops being taught as the app read model. It may remain as a
  substrate, debug, test, or expert acquisition primitive.
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
