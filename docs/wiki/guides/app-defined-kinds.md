---
title: "App-Defined Event Kinds: First-Class Support and Codegen"
slug: app-defined-kinds
topic: app-defined-kinds
summary: An app should be able to define its own made-up event kind â number, schema, builder â and have it be a first-class citizen in the app's own codebase, on pa
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
  - session:019f0dc3-5b56-79d1-a14b-5746c93e5879
---

# App-Defined Event Kinds: First-Class Support and Codegen

## Goal

An app should be able to define its own made-up event kind — number, schema, builder — and have it be a first-class citizen in the app's own codebase, on par with NMP's built-in kinds. This means no upstreaming to NMP, no hand-rolling NMP-generated boilerplate, and no raw/untyped fallback. App-defined kind support (tracked via #2408/#2413/#2414) has been pulled into v1 DX scope, not deferred post-v1.

First-class app-defined kinds require: the app declares its kind schema in its own source, the same codegen NMP runs emits typed builders and native bindings into the app's crate, the app's kind gets the same typed-contract and drift enforcement in the app's CI, and the app registers the ActionModule at its composition root to ride the generic construct→sign→route pipeline. <!-- [^898a4-f3f56] -->

<!-- citations: [^898a4-8826a] [^898a4-0a043] -->
## Current state

NMP's codegen pipeline for typed action builders is hardcoded to NMP's built-in NIP schemas only. The `ACTION_BUILDERS` const array registry in `nmp-codegen` defines which event kinds get generated typed builders, and it contains only NMP's built-in NIPs. There is no `--external-registry` or `--schema-dir` hook for apps to feed their own kind schemas into the generator. The `ActionModule` trait and `ActionRegistrar::register_action` are public, so apps can register custom action modules at composition time — but without generated typed builders. The intended end state is that NMP's codegen accepts an app-local registry and generates typed builders/bindings/drift gates from it, so app-defined kinds become first-class without upstreaming to NMP.

<!-- citations: [^898a4-53328] [^019f0-aa4b7] [^898a4-c6f07] -->
## Kind ownership boundary

An app's custom kind schema and builder live in the app's crate, not in NMP. App-private event kinds are an app-owned local static contract: the app owns `action-builders.json` and the `.fbs` schema next to its Rust crate, and NMP provides generated typed builders, bindings, and drift gates from that app-local registry. The app Rust crate owns `ActionPayload::decode`, validation, tag/content policy, and the `ActionModule` implementation. There is no runtime plugin system and no upstreaming requirement. The ownership test is whether the crate would be useful to a completely different Nostr app; if yes, it is app-owned and stays out of NMP. NMP does not require a bespoke write door per event kind — a kind is data, not a code path. What varies is who owns the kind and how it is routed, while signing is uniform across all kinds.

<!-- citations: [^898a4-2a15e] [^019f0-261ed] -->
## Routing and store semantics

NIP-29 owns only the h-tag routing concern, not kinds — the kinds filter was deliberately removed, and `GroupEventsProjection` reads consumer-declared kinds. The store derives replaceable, addressable, and ephemeral semantics from the event kind number range, not from any per-kind registration. <!-- [^898a4-2d7a6] -->

## Tracking

GitHub issue #2408 was filed to explore the design space for making app-defined event kinds first-class citizens. It is labeled `category:decision`, `area:codegen`, `area:architecture`, `status:needs-decision`, `priority:p2`. <!-- [^898a4-aaf36] -->

## App-private kind flow

The app-private kind flow proceeds end-to-end as follows. The app owns `action-builders.json` and the `.fbs` schema next to its Rust crate. `nmp-codegen` generates Swift, Kotlin, and TS typed builders from that app-local registry. Native and web call the generated builder and dispatch bytes through the normal NMP doorway. The app Rust crate owns `ActionPayload::decode`, validation, tag/content policy, and the `ActionModule`. NMP owns only the substrate after that: dispatch envelope, registration, signing, routing, publishing, and drift gates. <!-- [^019f0-b38df] -->

## Starter proof

The starter proof for app-private kinds must demonstrate the full end-to-end path: generated builder bytes flow into the app Rust `ActionPayload` decode, through the registered `ActionModule`, and result in a declared private event kind being published. <!-- [^019f0-a1c3c] -->
