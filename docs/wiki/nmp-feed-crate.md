---
title: NMP Feed Crate & Typed Wire Schema
slug: nmp-feed-crate
summary: The nmp-feed crate owns the typed wire for the outer FeedPage/FeedCursor/FeedWindowMetrics wrapper.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
---

# NMP Feed Crate & Typed Wire Schema

## Core Domain

The nmp-feed crate owns the typed wire for the outer FeedPage/FeedCursor/FeedWindowMetrics wrapper. The nmp-feed engine remains kind-agnostic per D0 doctrine, with kind-filtering supplied by the composition root rather than hardcoded in the generic engine. The Feed Registry is a thread-safe registry of `FeedController` trait objects keyed by string, supporting `register` and `load_older` operations.

<!-- citations: [^56db9-6] [^54ae9-17] [^855be-5] -->
## Parity Testing and Fixtures

JSON/generic-vs-typed parity fixtures must be added before any iOS/TUI/web host migration. Parity tests prove that typed decoding preserves serde projection semantics. [^56db9-7]

## Host Decoder Strategy

The TUI host prefers the typed payload path with a generic Value fallback during the window transition. Web and Android host decoders are explicitly deferred, with web following after TUI adoption and Android requiring a stale byte/string bridge fix first. [^56db9-8]
## See Also

