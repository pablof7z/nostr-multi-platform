---
title: NmpAppBuilder Typestate — Compile-Time Pre-Start Ordering
slug: nmp-app-builder-typestate
summary: `NmpAppBuilder` must use a typestate pattern with states `Unstarted` and `StorageSet`
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:4eb4e0e2-a9b3-4347-a92b-a073af7adfc0
---

# NmpAppBuilder Typestate — Compile-Time Pre-Start Ordering

## Typestate Pattern

`NmpAppBuilder` must use a typestate pattern with states `Unstarted` and `StorageSet`. The `start()` method only exists on `StorageSet` and consumes the builder, enforcing compile-time pre-start ordering constraints. [^4edd4-228]


NmpAppBuilder must use a typestate pattern with states Unstarted and StorageSet. The start() method only exists on StorageSet and consumes the builder, enforcing compile-time pre-start ordering constraints. Read-only apps use an empty Action enum rather than leaving implementers to deduce it from compiler errors. [^4eb4e-2]

## Kernel Bootstrap

The builder guide must include a Rust-native kernel bootstrap section (~30 lines in §19) showing how to start the NMP kernel from a Rust binary. [^4eb4e-3]
## See Also

