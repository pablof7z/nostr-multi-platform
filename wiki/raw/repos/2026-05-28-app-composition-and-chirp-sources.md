---
title: "App Composition and Chirp Wiring Sources 2026-05-28"
summary: "Source notes for nmp-app-template defaults, Chirp per-app registration, and OP-feed composition boundaries."
tags: [repo, app-composition, chirp, ffi]
source_type: repo-snapshot
repo: /Users/pablofernandez/Work/nostr-multi-platform
commit: 50ecae23b3587affa1ae167baa067a1e07b9a677
ingested: 2026-05-28
updated: 2026-05-28
---

# App Composition and Chirp Wiring Sources 2026-05-28

## Primary Source Files

- `crates/nmp-app-template/src/lib.rs`
- `crates/nmp-app-template/src/op_feed_defaults.rs`
- `apps/chirp/nmp-app-chirp/src/lib.rs`
- `apps/chirp/nmp-app-chirp/src/ffi/register.rs`
- `apps/chirp/nmp-app-chirp/src/ffi/snapshot.rs`
- `apps/chirp/nmp-app-chirp/tests/end_to_end.rs`
- `apps/chirp/nmp-app-chirp/tests/typed_feed_parity.rs`

## `register_defaults`

`nmp-app-template` is the canonical generic Nostr composition root. Its
`register_defaults` function wires common action modules, ingest parsers,
routing substrate, publish resolver, indexer-republish policy, coverage hook,
and runtime controllers onto an `AppHost`.

The template deliberately does not wire app-specific projections. It does not
own the C ABI and does not start the app lifecycle. It must run before
`nmp_app_start` so the kernel sees parsers, routing factories, and observers
before the first event arrives.

## Routing and Publish Injection

The routing factory installed by the template returns a
`GenericOutboxRouter` plus an `InMemoryMailboxCache`. The kind:10002 parser is
registered against the same cache, so parser writes and router/planner reads
share one source of truth. `nmp-core` holds the traits and actor slots; the
router crate holds the algorithm and cache implementation.

The publish resolver is likewise injected through a factory. Production uses
`nmp_router::Nip65OutboxResolver`, while the kernel default is fail-closed.

## Chirp Registration

`nmp_app_chirp_register` calls `nmp_app_template::register_defaults`, then
adds Chirp-specific registrations: NIP-29 actions, visible note relation
actions, optional wallet runtime, zap aggregates, and the Chirp home timeline
projection.

The home feed currently registers a `ModularTimelineProjection` under
`"nmp.feed.home"` as both a feed controller and a typed snapshot sidecar
producer. The typed sidecar encodes the same current bounded window as the
generic feed projection.

## OP Feed Defaults

`register_op_feed_defaults` exists as an OP-centric feed composition helper,
but it is not called from `register_defaults`. Its docs say it wires
`ActiveFollowSet`, an `OpFeedEngine`, a claim sink, an event lookup closure,
and account-switch reset behavior. It explicitly does not register duplicate
per-follow `LogicalInterest`s, because the kernel still owns the existing
follow-feed subscription expansion.

## Legacy Chirp Snapshot Export

`nmp_app_chirp_snapshot` still serializes a `ModularTimelineSnapshot` as JSON,
but its doc comment marks it diagnostics-only. Runtime hosts are expected to
consume the `"nmp.feed.home"` projection from the update stream.

## Authority Notes

For current behavior, prefer the source files above over design prose. Design
docs may describe a desired OP-feed composition or migration order; this raw
note records what the checked code on this branch actually wires.
