---
title: LMDB Diagnostics — Corruption Counters and Open-Failure Surfacing
slug: lmdb-corruption-and-diagnostics
summary: LMDB now surfaces orphan index entries and open-failure via typed diagnostics (StoreAnomalySnapshot, StoreUnavailable) instead of silently degrading to in-memory or swallowing errors.
tags:
  - lmdb
  - store
  - v67
  - v69
  - diagnostics
  - corruption
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# LMDB Diagnostics — Corruption Counters and Open-Failure Surfacing

> LMDB now surfaces orphan index entries and open-failure via typed diagnostics (StoreAnomalySnapshot, StoreUnavailable) instead of silently degrading to in-memory or swallowing errors.

## V-69: LMDB Orphan-Index Corruption

In `crates/nmp-nostr-lmdb/src/store/lmdb/mod.rs`, the `query_by_scraping` function previously used `.ok()??` on the `get_event_by_id` secondary lookup. This double-swallow discarded both `Ok(None)` (dangling index entry) and `Err(_)` (undeserializable event row) silently.

Fix:
- `Arc<AtomicU64>` fields `anomaly_orphan_index_entries` and `anomaly_unresolvable_events` added to `Lmdb` (shared across all clones via `Arc`)
- `StoreAnomalySnapshot { orphan_index_entries, unresolvable_events }` struct added as public type
- `Lmdb::store_anomaly_snapshot()` accessor returns current counter values
- The `.ok()??` replaced with an explicit `match` emitting `tracing::warn!` with hex-encoded key on each error class
- Both counters initialized in `open_databases_on_env` (the single constructor chokepoint used by both `Lmdb::new` and `Lmdb::with_env`) [^42908-37]

## V-67: LMDB Silent Degradation on Open Failure

Previously, a failed LMDB `open` silently fell back to an in-memory store with no observable signal to the host (`crates/nmp-core/src/kernel/mod.rs`, V-67). The fix surfaces a typed `StoreUnavailable` diagnostic so shells can detect LMDB initialization failure rather than receiving silently degraded behavior. [^42908-38]

## See Also
- [[kernel-boot-initial-emit-guarantee|Kernel Boot Initial Emit — Guaranteed Post-Start Snapshot Frame]] — related guide

