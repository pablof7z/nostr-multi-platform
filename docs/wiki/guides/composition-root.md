---
title: Explicit Composition Root and register_defaults Elimination
slug: composition-root
topic: composition-root
summary: Per ADR-0069, the composition root requires `register_defaults()` to be dead as a production path everywhere â in the starter, gallery, and browser
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:019f0dc3-5b56-79d1-a14b-5746c93e5879
---

# Explicit Composition Root and register_defaults Elimination

## Production Path Enforcement

Per ADR-0069, the composition root requires `register_defaults()` to be dead as a production path everywhere — in the starter, gallery, and browser. This is enforced by a CI ratchet that prevents any new live call site from being introduced.

The 'composition root' is the explicit build-time wiring where each app target (starter, gallery, browser production) composes its dependencies using builder methods instead of `register_defaults()`. A production app's Rust composition root must explicitly install substrate, reusable Nostr protocol features, app-owned product features, shell capability contracts, then start — `register_defaults()` is not production app architecture.

`nmp-defaults` may survive only as a reusable installer library, never owning seed follows, bootstrap relay brands, signer permission defaults, or onboarding/product policy.

Anything a second platform would have to reimplement to stay correct — relay choice, signer choice, tag mutation, publish retry, queue truth, nav meaning — belongs in Rust.

Doc and vocabulary ratchets are pro-migration: they exist to stop old `register_defaults` and raw-projection vocabulary from creeping back into docs.

<!-- citations: [^898a4-df88b] [^3c942-9b54c] [^3c942-3e07a] [^3c942-78b3e] [^898a4-2f6af] -->
## Explicit Composition Root and register_defaults Elimination

## Production Path Enforcement

Per ADR-0069, the composition root requires `register_defaults()` to be dead as a production path everywhere — in the starter, gallery, and browser. This is enforced by a CI ratchet that prevents any new live call site from being introduced.

A production app's Rust composition root must explicitly install substrate, reusable Nostr protocol features, app-owned product features, shell capability contracts, then start — `register_defaults()` is not production app architecture.

`nmp-defaults` may survive only as a reusable installer library, never owning seed follows, bootstrap relay brands, signer permission defaults, or onboarding/product policy.

Anything a second platform would have to reimplement to stay correct — relay choice, signer choice, tag mutation, publish retry, queue truth, nav meaning — belongs in Rust.

## Feature as the Composition Unit

An NMP app is constructed by installing explicit, named feature bundles rather than calling a magic `register_defaults()` function. A Feature is the main composition unit: it bundles typed views/read models, typed commands/writes, subscription demand, ingest parsers, capability needs, and projection encoders. The composition root wires these features together explicitly.

## What App Developers Should Know

App developers should know which feature bundles their app installs, which LiveQuery/read session a screen opens, which typed projection the screen renders, which typed commands publish events, when to close query handles, and when a query is default outbox-routed vs explicitly relay-pinned.

App developers should not need to know about SnapshotRegistry, projection tiers, muted observers, replay shapes, relay fanout details, NIP-65 mailbox routing internals, open_interest plumbing, FlatBuffer sidecar mechanics, or cache/store replay mechanics.

## Kernel Decomposition

The Kernel should not remain a god object owning store, relay state, projection registries, action ledger, publish engine, observers, auth, accounts, provenance, cache serves, pull cursors, lifecycle, and side registries as one enormous mental object. SnapshotRegistry should be split into real owners: projection registry, projection delivery contract, tick observers, and feed-author helpers, keeping the same facade with simpler internals.

## Internal Complexity Budget

Internal complexity is justified only if it protects a real invariant: replay-before-live, privacy, route provenance, bounded FFI, no polling, signer safety, teardown, or cross-platform consistency. <!-- [^019f0-22da8] -->
