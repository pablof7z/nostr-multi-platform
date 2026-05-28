---
title: "App Composition and Chirp Wiring"
summary: "How generic NMP defaults, Chirp per-app Rust glue, projections, and typed feed sidecars are composed before the kernel starts."
tags: [app-composition, chirp, ffi, projections]
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
volatility: warm
confidence: high
sources:
  - "raw/repos/2026-05-28-app-composition-and-chirp-sources.md"
  - "raw/repos/2026-05-28-source-map.md"
---

# App Composition and Chirp Wiring

NMP apps are assembled at a Rust composition boundary before the actor starts.
The composition root registers generic protocol modules, substrate factories,
runtime controllers, and app-specific projections. The native shell should not
discover these pieces dynamically or recreate their policy.

## Generic Defaults

`nmp-app-template::register_defaults` is the generic Nostr-app wiring point.
It installs common action modules, kind parsers, routing substrate, publish
resolver, indexer-republish policy, coverage hooks, and runtime controllers.

The most important ownership detail is the routing cache: the template creates
one `InMemoryMailboxCache`, registers the kind:10002 parser as its writer, and
passes the same cache through the routing-substrate factory. That preserves D4:
the parser writes mailbox facts once, and router/planner consumers read the
same fact stream.

The template intentionally does not start the app, expose C ABI symbols, or
register app-specific projections. It is reusable composition, not product
identity.

## Chirp's Extra Layer

`nmp_app_chirp_register` wraps the generic defaults with Chirp-specific Rust
glue:

- NIP-29 actions and group projections;
- visible note relation actions from `nmp-nip01`;
- optional NIP-47 wallet runtime;
- zap aggregate projection under `nmp.nip57.zaps`;
- home-feed projection under `nmp.feed.home`;
- typed home-feed sidecar emission for the same feed window.

The iOS shell links the aggregate app crate, but the grouping, relation, zap,
feed, and routing decisions remain in Rust.

## `nmp.feed.home`

The current Chirp home feed is registered from
`ModularTimelineProjection`. It is exposed in two ways:

- as a generic feed controller under `"nmp.feed.home"`;
- as a typed snapshot projection with schema id `nmp.nip01.timeline`.

Both read from the same projection instance. This matters because a typed
transport sidecar must not become a second source of feed truth. It is another
encoding of the bounded current window.

## OP-Feed Defaults

`register_op_feed_defaults` is a separate helper for the OP-centric feed
engine. It wires an `ActiveFollowSet`, `OpFeedEngine`, event lookup closure,
claim sink, and account-switch reset callback.

It is not called by `register_defaults`. It also deliberately does not register
per-follow interests, because the kernel still owns the existing follow-feed
subscription expansion. Adding the engine and duplicating the subscription
expansion would produce duplicate REQs.

## Pull Snapshot Is Diagnostics

`nmp_app_chirp_snapshot` still exists, but its source marks it
diagnostics-only. Runtime hosts should consume the normal update stream and
the registered projections instead of pulling a JSON timeline snapshot through
the Chirp handle.

## See Also

- [[op-feed-and-typed-projections|OP Feed and Typed Projections]] ([OP Feed and Typed Projections](op-feed-and-typed-projections.md))
- [[runtime-update-transport|Runtime Update Transport]] ([Runtime Update Transport](runtime-update-transport.md))
- [[crate-boundaries-and-module-ownership|Crate Boundaries and Module Ownership]] ([Crate Boundaries and Module Ownership](crate-boundaries-and-module-ownership.md))

## Sources

- [App Composition and Chirp Wiring Sources](../../raw/repos/2026-05-28-app-composition-and-chirp-sources.md)
- [NMP Source Map 2026-05-28](../../raw/repos/2026-05-28-source-map.md)
