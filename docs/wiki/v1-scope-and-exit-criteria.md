---
title: v1 Scope, Exit Criteria, and Platform Targets
slug: v1-scope-and-exit-criteria
summary: v1 targets iOS, macOS, and Android only. Marmot/MLS and NWC+zaps are in scope. Wasm/IndexedDB is deferred post-v1.
tags:
  - v1
  - release
  - scope
  - wasm
  - marmot
  - nwc
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# v1 Scope, Exit Criteria, and Platform Targets

> v1 targets iOS, macOS, and Android only. Marmot/MLS and NWC+zaps are in scope. Wasm/IndexedDB is deferred post-v1.

## Platform Targets

v1 targets **iOS, macOS, and Android only**. Wasm and browser platforms are explicitly deferred to post-v1. [^42908-5]

## In-Scope Features (v1)

- **Marmot/MLS** (group messaging via MLS protocol) — in scope for v1; this decision was already made and largely executed in the codebase. (PD-041 resolved.)
- **NWC + zaps** (NIP-47 wallet connect + NIP-57 zaps) — in scope for v1. (PD-041 resolved.) [^42908-6]

## Deferred Post-v1

- **F-01 (IndexedDB persistence for wasm)**: deferred; wasm is not a v1 platform target.
- **V-84 / V-85** (wiring typed iOS/Android NFCT decoders into render): tracked as post-v1 tail items at the time of the V-80 landing, though V-84's iOS typed NOFS+NFCT path was subsequently implemented in PR #762. [^42908-7]

## Open v1 Blockers

- **F-02 — DM cold-start receive-side verification**: Rust pipeline verified; device-level QA on a live relay is still needed.
- **F-04 — Zap E2E round-trip against live NWC**: the full chain is wired but has no automated verification against a live NWC endpoint. A live-relay / live-NWC validation harness does not yet exist.
- **F-05 — Codegen Swift Decodables** (tagged enums, legacy_default): ~17–20% coverage; Stage 3 remainder is effectively post-v1. [^42908-8]

## See Also

