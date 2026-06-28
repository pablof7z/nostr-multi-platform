# New Architecture Sketch

> **Status:** Proposed design capture for issues #2313 and #2316. This is not a
> shipped API contract. It records the desired shape so an ADR can settle final
> naming, migration order, and implementation details before code changes.

This sketch describes how an NMP app should feel to build from a developer
perspective, and what NMP should do internally to make that simple surface true.
The core idea is:

```text
install features
  -> open live queries for screens
  -> render typed Rust-owned state
  -> construct event drafts
  -> sign with a selected signer
  -> publish through Rust-owned routing policy
```

The framework may stay internally complex where Nostr requires it. The app API
must not require developers to manually compose raw interests, observers, cache
replay, dynamic dependency sources, projection sidecars, snapshot ticks, and
teardown recipes.

## Documents

- [App Model](01-app-model.md) explains how an app is assembled, what feature
  bundles provide, and what the developer should and should not need to know.
- [Live Queries](02-live-queries.md) explains how screens subscribe to data,
  including `ObservedProjection` and `ReducedSource`.
- [Write Flow](03-write-flow.md) explains the split between event construction,
  event signing, and event publishing.
- [Internal Machinery](04-internal-machinery.md) explains what NMP does under
  the hood for reads, writes, actors, stores, and follow-up ADR work.

## North Star

An NMP app should be understandable from a small set of concepts:

- A Rust composition root installs explicit feature bundles.
- Screens open live queries for the data they render.
- Shells render typed projections produced by Rust.
- Event construction is composable and protocol-aware.
- Signing is explicit enough to choose a signer, but Rust-owned enough to keep
  native backends interchangeable.
- Publishing applies route policy, protocol pins, delivery, retry, and status in
  Rust.
- Native and web shells render UI and execute capabilities. They do not own
  protocol correctness, durable state, relay planning, or product logic.

## Terms Used Here

The names are deliberately provisional:

- `LiveQuery` means the app-facing live read lifecycle a screen opens.
- `ObservedProjection` means the internal safe pattern for replaying cached
  events into a scoped projection before accepting future live events.
- `ReducedSource` means the internal pattern for dynamic query inputs derived
  from other events or account state.
- `EventDraft` means unsigned event bytes plus protocol context that may still
  be mutated before signing.
- `PublishContext` means route, privacy, and protocol policy that travels with a
  draft or signed event until publish is complete.

The ADR can rename any of these. The shape is the important part.
