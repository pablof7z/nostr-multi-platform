---
title: EventStore Trait and Backends
slug: eventstore-trait
topic: data-persistence
summary: `EventStore` (`crates/nmp-store/src/events.rs`) is the unified interface for event persistence
tags:
  - capture
volatility: warm
confidence: high
created: 2026-06-18
updated: 2026-06-18
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
---

# EventStore Trait and Backends

## The EventStore Trait

`EventStore` (`crates/nmp-store/src/events.rs`) is the unified interface for event persistence. It exposes:

- `insert(event, relay_url, now)` — atomically stores an event plus all secondary state (provenance, LRU, expiry index, tombstone enforcement) in a single `RwTxn`. Returns a typed `InsertOutcome` (`Inserted`, `Replaced`, `Duplicate`, `Tombstoned`, …).
- `query_visit(query, limit, visitor)` — streaming visit over store results ordered newest-first (`created_at DESC, id ASC`). The visitor returns `ControlFlow<()>`; `Break` terminates early. This is the primary read path for cache-serve.
- `query(query, limit)` — compatibility wrapper over `query_visit` that collects into a `Vec<StoredEvent>`.
- `relay_kind_coverage(relay_url)` — returns `Vec<u32>` of kind numbers for which the relay has at least one event. Default impl returns an empty vec.
- `relay_kind_count(relay_url, kind)` — returns `u64` count of events for a given relay+kind pair. Default impl returns zero.
- Relay coverage, GC, tombstone, and provenance accessors.

All trait methods have default implementations that compile on non-LMDB targets; the LMDB backend overrides the hot paths in `store_impl.rs`.

<!-- citations: [^129d2-52] -->
## MemEventStore

Always compiled. Used in all tests, WASM builds, and cold-start fallback when LMDB fails to open. Backed by an in-memory `BTreeMap`; full-table-scan on query. O(N) per query — not a production path.

Maintains `relay_kind`: a `HashMap<RelayUrl, HashMap<u32, BTreeSet<String>>>` tracking which kinds each relay carries. Private-kind-gated `relay_kind_add`/`relay_kind_remove` helpers are called at the same sites as `relay_index_add`/`relay_index_remove`, keeping the map in parity with the relay index.

The `for_each_backend!` macro in `StoreHarness` runs a test against both `MemEventStore` and `LmdbEventStore`. Integration tests that exercise LMDB directly use `StoreHarness::lmdb()`. For integration tests, `StoreHarness` uses `VerifiedEvent::from_raw_unchecked` to produce synthetic signatures rather than `nostr::Keys` signing.

<!-- citations: [^129d2-53] [^129d2-109] -->
## LmdbEventStore

Compiled always; functional only with `--features lmdb-backend`. Uses the NMP fork of `nostrdb`'s LMDB env (`crates/nmp-nostr-lmdb`). Secondary state (provenance, tombstones, LRU, expiry, relay scores, coverage, relay index, interaction counters) lives in NMP-owned sub-databases committed atomically with the event write.

The fork's 11 sub-databases are always open. NMP adds its own. `NMP_ADDITIONAL_DBS` (currently 10) sets the `max_dbs` headroom; bump it when adding a sub-db.

Issue #1519 adds the `nmp-interaction-counters` sub-db. Its key format is `target_event_id(32) || counter_kind(1) → u64` big-endian count. A shared classifier is provided in `crates/nmp-store/src/interaction.rs`.

<!-- citations: [^129d2-88] -->
## StorageBackend Enum

`crates/nmp-store/src/lib.rs`:

```rust
pub enum StorageBackend {
    Memory,
    Lmdb { path: PathBuf },
}
pub fn open_event_store(backend: &StorageBackend) -> Result<Box<dyn EventStore>, StoreError>
```

`build_event_store` (`crates/nmp-core/src/kernel/store_init.rs`) constructs the backend: if `lmdb-backend` feature is enabled AND a `data_path` is provided, it opens `LmdbEventStore`. If open fails, it falls back to `MemEventStore` and records the failure reason in `Kernel::store_open_failure` (projected into every snapshot's `store_open_failure` field so the host can surface a diagnostic toast).

## Single-Writer Invariant

ADR-0011: one `RwTxn` per insert. NMP never opens a second writer. The fork's `deleted_ids`/`deleted_coordinates` sub-databases are left empty — tombstone enforcement lives in NMP's own `tombstones`/`addr_tombstones` sub-dbs (D4 — single-writer-per-fact).
