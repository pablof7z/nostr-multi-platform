---
title: NIP-25 Reaction Ownership
slug: nip25-reaction-ownership
topic: crate-architecture
summary: nmp-nip25 is the single owner of public kind 7 reaction actions and projection; nmp-nip02 retains only compatibility re-exports for the old ReactModule type nam
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-19
updated: 2026-06-19
verified: 2026-06-19
compiled-from: conversation
sources:
  - session:019edcba-b578-71f3-be33-f670962f11a7
---

# NIP-25 Reaction Ownership

## Reaction Ownership & Module Boundaries

nmp-nip25 is the single owner of public kind 7 reaction actions and projection; nmp-nip02 retains only compatibility re-exports for the old ReactModule type names. <!-- [^019ed-155] -->

The nmp.nip25 react action validation must reject short placeholder event IDs and require valid 64-hex IDs. <!-- [^019ed-156] -->

Public NIP-25 reaction publishing must reach core only as a generic unsigned event through the publish one-door, not through a dedicated ActorCommand::React path. <!-- [^019ed-157] -->

The NIP-25 projection must be exposed as a bounded KernelEventObserver plus Rust snapshot API, with FlatBuffers/host projection wiring deferred to a later issue. <!-- [^019ed-158] -->
