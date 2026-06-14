---
title: WASM Persistence
slug: wasm-persistence
topic: data-persistence
summary: The WASM runtime must use OPFS SyncAccessHandle-backed SQLite as the primary persistence backend, not IndexedDB, because EventStore is a synchronous trait and I
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-14
updated: 2026-06-14
verified: 2026-06-14
compiled-from: conversation
sources:
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
---

# WASM Persistence

## Persistence Backend

The WASM runtime must use OPFS SyncAccessHandle-backed SQLite as the primary persistence backend, not IndexedDB, because EventStore is a synchronous trait and IndexedDB is async-only. <!-- [^bf035-172] -->

Persistence is an optional substrate slot — the kernel must boot on MemEventStore and report an honest 'not persisting' degraded mode when OPFS is unavailable (private browsing, quota denial, Safari without sync access handles). <!-- [^bf035-173] -->

The offline action queue must be durable in OPFS-SQLite, not held in in-memory actor state, because actions dispatched offline must survive reload. <!-- [^bf035-174] -->

Unsafe Send+Sync on the WASM store wrapper is sound because the Worker is single-threaded, and this must not be done by cfg-relaxing the bound in the core crate. <!-- [^bf035-175] -->

## Actor Model on WASM

The Worker's event loop IS the actor on WASM — direct synchronous KernelReducer calls are the actor loop, with async capabilities re-entering via spawn_local/callbacks, not a ported OS-thread+flume+tokio actor. <!-- [^bf035-176] -->

## Staged Integration

Stage 5 (persistence OPFS-SQLite) is the store-injection seam — pure plumbing, zero behavior change on native, and lands with an in-memory store first; Stage 6 (the actual SQLite backend) carries nearly all the risk and must be gated on a Playwright-driven Worker test harness and a cfg-gated optional dep. <!-- [^bf035-177] -->
