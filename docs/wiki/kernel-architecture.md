---
title: Kernel Architecture and Reducer Loop
slug: kernel-architecture
topic: code-architecture
summary: "NMP's kernel implements an Elm-style reducer loop as the pure event-processing core, with five architectural tiers: Kernel struct, actor loop, substrate layer,"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-12
updated: 2026-06-12
verified: 2026-06-12
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# Kernel Architecture and Reducer Loop

## Kernel Architecture

NMP's kernel implements an Elm-style reducer loop as the pure event-processing core, with five architectural tiers: Kernel struct, actor loop, substrate layer, projection/snapshot layer, and KernelReducer for WASM consumers. The rust-nostr TagStandard OnceCell<Option<TagStandard>> per-tag memory overhead should be quantified before it impacts NMP kernel RAM-eviction tuning.

<!-- citations: [^da6b1-104] [^954c5-18] -->
