---
title: Explicit Composition Root and register_defaults Elimination
slug: composition-root
topic: composition-root
summary: Per ADR-0069, production apps compose named owners directly
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-04
verified: 2026-07-02
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:019f0dc3-5b56-79d1-a14b-5746c93e5879
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:fb992e80-b32b-4673-b2c2-40e8044504ee
---

# Explicit Composition Root and register_defaults Elimination

## Production Path Enforcement

Per ADR-0069, production apps compose named owners directly. A composition root installs substrate, reusable Nostr protocol features, app-owned product features, shell capability contracts, typed outputs, and outbound identity metadata explicitly before the app starts.

`register_defaults()` and `nmp-defaults` are deleted production and scaffold architecture. Do not replace them with a hidden preset, compatibility bundle, or renamed default package. A reviewer must be able to read the app root and see which owners and policy knobs are installed.

Compat aliases are never permitted, even as migration scaffolding. The project enforces a zero-tolerance no-compat-aliases rule: no temporary rename, re-export, or shim may bridge old names to new owners. Old APIs are removed outright; callers are updated to the explicit composition root in the same change.

Anything a second platform would have to reimplement to stay correct - relay choice, signer choice, tag mutation, publish retry, queue truth, or navigation meaning - belongs in Rust.

Doc and vocabulary ratchets prevent old hidden-composition and raw-read concepts from returning to current docs or templates.

The desktop (iced) gallery composition root injects the `Always` `AdResolutionPolicy` so that moment-1 AD resolution fires; the default `NeverAutoResolve` shows nothing and must not be used in that gallery root. <!-- [^fb992-1d013] -->

<!-- citations: [^91a86-87c34] -->
## Feature as the Composition Unit

An NMP app is constructed by installing explicit, named owners. A reusable
protocol crate owns generic Nostr mechanics. An app crate owns product nouns,
ranking, onboarding, relay brands, and other proprietary policy. The
composition root wires these owners together explicitly.

A feature owner may provide typed read-session helpers, write intents or draft
builders, action handlers, parsers, capability contracts, and typed output
encoders. It is installed by name; it is not a hidden bundle of product policy.

## What App Developers Should Know

App developers should know which owners their app installs, which typed read
session or feed helper a screen opens, which typed output the screen renders,
which typed write intent publishes events, when handles close, and when routing
is default outbox versus an audited opt-out.

App developers should not need to wire snapshot registries, raw acquisition
interests, observed-projection sinks, reducer names, source effects, relay fanout
details, mailbox-routing internals, FlatBuffer transport details, or cache/store
replay mechanics for production screens.

## Kernel Decomposition

Kernel internals may remain complex only behind typed app-facing surfaces.
Internal owners should stay cohesive: store, relay state, action ledger, publish
engine, auth, accounts, provenance, cache serving, pull cursors, lifecycle, and
output delivery should not leak as app composition concepts.

## Internal Complexity Budget

Internal complexity is justified only when it protects a current invariant:
replay-before-live, privacy, route provenance, bounded FFI, no polling, signer
safety, teardown, or cross-platform consistency.
