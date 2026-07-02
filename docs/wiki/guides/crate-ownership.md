---
title: NMP Crate Ownership and Helper Policy
slug: crate-ownership
topic: crate-ownership
summary: Anything a second platform would have to reimplement to stay correct â relay choice, signer choice, tag mutation, publish retry, queue truth, nav meaning â
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:019f0dc3-5b56-79d1-a14b-5746c93e5879
---

# NMP Crate Ownership and Helper Policy

## Platform Shell vs. Rust Core Boundary

Anything a second platform would have to reimplement to stay correct — relay choice, signer choice, tag mutation, publish retry, queue truth, nav meaning — belongs in Rust, not in platform shells. Widgets, AppIntents, CarPlay, and Live Activities must not own parallel queues or publish models. <!-- [^3c942-39fb3] -->

## Downstream App Requests

A request from one app is evidence, not permission to specialize the framework; do not add app-named helpers, bespoke publish/read commands, hard-coded product defaults, operator policy, compatibility shims, or quick shared-crate workarounds just because a consuming app needs them. NMP does not require a bespoke write door per event kind: a kind is data, not a code path. What varies is who owns the kind and how it is routed, while signing is uniform. An app inventing its own kind (e.g. kind 232123) with its own tag schema declares it as an app-owned product feature at its composition root, registering its own ActionModule; NMP never learns what the kind means — it just signs and routes it. <!-- [^898a4-d80b6] -->

## NMP Crate Ownership and Helper Policy

NMP crates (crates/) provide reusable Nostr infrastructure that any Nostr application, or a meaningful subset of Nostr applications, could use directly. The ownership test for where a feature lives is: 'would this crate be useful to a completely different Nostr app?' If yes → NMP crate; if app-specific proprietary domain → app crate. App Rust crates (apps/<app>/) hold the Rust side of features specific to that application's domain that would not generalize to other Nostr apps; NMP does not accumulate app-specific logic. The line for crate placement is not protocol vs. product but generic Nostr building block vs. this app's proprietary domain — a product-level feature like NIP-29 group chat belongs in an NMP crate if other Nostr apps would use it. Protocol-owned reusable projections like follow-list live in nmp-nip02, not app FFI crates. <!-- [^898a4-d605b] -->
