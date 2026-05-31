---
title: NmpAppBuilder Typestate & Compile-Time Pre-Start Ordering
slug: nmp-app-builder-typestate
summary: "The NmpAppBuilder typestate enforces compile-time pre-start ordering: an Unstarted builder must call `.storage_path()` or `.in_memory()` to transition to Storag"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# NmpAppBuilder Typestate & Compile-Time Pre-Start Ordering

## Typestate Flow

The NmpAppBuilder typestate enforces compile-time pre-start ordering: an Unstarted builder must call `.storage_path()` or `.in_memory()` to transition to StorageSet. The `start()` method only exists on StorageSet and consumes the builder. [^4edd4-16]


The `mem::forget` pointer transfer in NmpAppBuilder is memory-safe: the Copy pointer is extracted before forget, Drop fires exactly once, and panic-safety holds. [^4edd4-17]
## See Also

