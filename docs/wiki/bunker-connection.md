---
title: Bunker Connection State
slug: bunker-connection
topic: bunker-connection
summary: Bunker connection state has a full typed FlatBuffers pipeline (schema, Rust codec, Swift/Android decoders, UI indicators on both platforms) wired through the Ti
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Bunker Connection State

## Architecture

The bunker connection state projection (KBCS) is emitted through the full production chain (Pool → broker → FFI → actor slot → typed sidecar → UI) and surfaced as a green dot/amber spinner/red warning on both iOS and Android.

<!-- citations: [^da6b1-2] [^da6b1-24] [^da6b1-43] [^da6b1-59] [^da6b1-96] -->
