---
title: LMDB Backend Stub
slug: lmdb-backend-stub
summary: LmdbEventStore is non-functional even with the `--features lmdb-backend` flag enabled; every trait method returns `not_enabled()` unconditionally.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:7f0f0c78-d1aa-49db-b659-c9cf49827117
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:c0765978-d977-4400-8274-96df7682b126
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# LMDB Backend Stub

## LmdbEventStore Functionality

LmdbEventStore is a stub with all 25 trait methods returning errors; there is no actual heed or LMDB crate dependency in any Cargo.toml. LMDB's single-writer constraint is acceptable for Nostr event stores because ingestion is bursty but write-bound throughput is not the bottleneck — reads and queries dominate UI rendering. Known implementation gaps in the stub: gc_step never evicts because LRU eviction is not implemented, kernel init surfaces an LMDB open failure as a typed StoreUnavailable diagnostic instead of falling back silently to an in-memory store, and ok()?? / filter_map(res.ok()) silently swallows index-corruption errors producing incomplete query results. Relay browsing reverse-index methods are honestly stubbed with NotSupported rather than using the V-17 Vec::new() silent-empty anti-pattern.

<!-- citations: [^7f0f0-11] [^57528-8] [^c0765-2] [^cd2b6-9] [^42908-9] [^4edd4-12] -->
## See Also

