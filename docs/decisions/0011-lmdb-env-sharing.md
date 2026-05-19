# ADR 0011: NMP owns the LMDB environment and injects it into nostr-lmdb

**Date:** 2026-05-18
**Status:** accepted
**Resolves:** `docs/design/lmdb-schema.md` §13 open question 1
**Depends on:** ADR-0009 (kernel boundary), ADR-0003 (working-set memory)

## Context

The M3 persistence layer wraps `nostr-lmdb` (the upstream Rust crate) behind NMP's own `EventStore` trait. The implementation holds both (a) a `NostrLMDB` for canonical event storage and (b) NMP-owned secondary LMDB sub-databases (provenance, watermarks, claims, domain rows, tombstones).

The core atomicity requirement is: **every `insert()` call must commit event + provenance + all NMP secondary index entries in a single LMDB write transaction.** This is mandatory because:

- A crash between the primary write and the secondary writes would leave the index in a state inconsistent with the primary store — the next `scan_by_author_kind` could return stale or missing results.
- A crash between the migration data writes and the `_meta` version write would re-run an already-committed migration step, potentially corrupting migrated data.

Per doctrine D6: "errors never cross FFI as exceptions." The corollary is that the *store itself* must never silently produce an incorrect answer because of partial writes. Any scenario where two separate `lmdb::Environment` handles could each commit independently would violate this — we would have no way to roll back one side when the other fails.

**Why this matters (D6):** if the store returns wrong data because of a partial write, that wrong data eventually reaches FFI as an `AppUpdate`. There is no way to signal "this snapshot is partially corrupt" through a `Result<T, StoreError>` once the data is already in the `AppUpdate` struct. The only safe design is to prevent partial writes at the store layer.

## Decision

**NMP owns the `lmdb::Environment` and injects it into `nostr-lmdb`.**

Concretely:

1. `LmdbEventStore::open(path)` calls `lmdb::Environment::open(path, ...)` and receives the sole `Environment` handle.
2. This `Environment` is passed to `NostrLMDB` via a new constructor `NostrLMDB::with_env(env: Arc<lmdb::Environment>) -> Self` (an upstream PR to `nostr-lmdb`).
3. Both `nostr-lmdb`'s sub-databases and NMP's sub-databases are opened under this single `Environment`.
4. `insert()` opens one `lmdb::RwTxn`, calls `nostr_lmdb.save_event_in_txn(txn, &event)` plus NMP's secondary writes, then commits once. Either all writes land or none do.
5. Migration steps open one `RwTxn` per step and commit the data writes and the `_meta` version bump together (see `lmdb/watermarks.md` §4.2).

## How it works

### Upstream PR

The upstream PR to `nostr-lmdb` adds:

```rust
impl NostrLMDB {
    /// Construct with a caller-owned Environment. The caller is responsible for
    /// keeping the Environment alive as long as this NostrLMDB is alive.
    pub fn with_env(env: Arc<lmdb::Environment>, opts: NostrLMDBOptions) -> Result<Self, Error>;

    /// Insert an event inside an existing write transaction. The caller commits.
    pub fn save_event_in_txn<'txn>(
        &self,
        txn: &mut lmdb::RwTxn<'txn>,
        event: &nostr::Event,
    ) -> Result<SaveEventStatus, Error>;
}
```

The PR is straightforward: the existing `NostrLMDB` already opens one `Environment` internally; the change is to accept an externally-owned `Arc<Environment>` instead and expose the txn-scoped write primitive.

### Interim fallback

If the upstream PR is not merged within the M3 implementation window, we use a pinned fork of `nostr-lmdb` at the commit we need (adding a `[patch.crates-io]` entry in the workspace `Cargo.toml`). The fork carries only the two additions above; no other changes. The fork reference is replaced with the upstream version once the PR lands.

### Two-phase-write fallback (if environment sharing is impossible)

If the upstream crate architecture fundamentally prevents sharing the `Environment` (e.g., because it calls `Environment::open` with exclusive flags), the fallback is:

1. Two separate `Environment` handles: one for `nostr-lmdb`'s sub-dbs, one for NMP's sub-dbs.
2. `insert()` writes to `nostr-lmdb` first (primary + upstream indexes), then to NMP's sub-dbs in a second transaction.
3. On crash recovery at startup, a consistency check compares the two environments' primary event sets against NMP's secondary indexes and replays any NMP writes that didn't land (a WAL-style forward-recovery scan limited to events inserted in the last write window, identified by the `_wal_pending` sub-db).

This fallback is significantly more complex and has a measurable write-amplification cost. It is not the preferred design and should only be used if the upstream PR path is closed. The check `StoreHealth::consistency_status` (exposed to the diagnostics bridge per ADR-0007) surfaces any out-of-sync state detected at startup so it is always visible to developers.

## Consequences

**Positive:**
- Single-commit atomicity for all `insert()` writes. No half-state visible to any reader.
- Simpler recovery: LMDB's built-in crash-recovery (WAL + mmap) is sufficient; no NMP-side recovery logic needed.
- Migration steps are atomic by construction (§4.2 of `lmdb/watermarks.md`).

**Negative:**
- Requires an upstream PR to `nostr-lmdb`. If the PR is rejected and the crate owner doesn't expose an environment-injection constructor, we must fork. The fork is a maintenance surface.
- NMP becomes the LMDB environment owner, which means it is responsible for tuning `mapsize`, `max_dbs`, and `max_readers`. These were previously handled by `nostr-lmdb` internally. The defaults used by `nostr-lmdb` are well-chosen for Nostr workloads; NMP adopts them as its starting point and adjusts only when benchmarks show a regression.

## Alternatives considered

**A. Let `nostr-lmdb` own the environment; use a two-phase write with WAL-recovery.** Described above as the fallback. Rejected as the primary design because of the added complexity, the recovery-window ambiguity, and the write-amplification cost.

**B. Replace `nostr-lmdb` with a hand-rolled LMDB layer.** Rejected in the master doc §1 (see "Rejected alternatives"). Reinvents 2 000+ LOC of battle-tested NIP-09 / replaceable event logic at high bug risk. Not justified.

**C. Use SQLite (or another store) that supports multi-table transactions natively.** SQLite supports cross-table ACID transactions without environment sharing issues. Rejected because (a) the iOS-disk-WAL fsync cost at our 10k-event hot working set is higher than LMDB's mmap model, (b) `nostr-lmdb` gives us NIP-77 negentropy integration we would have to re-implement, (c) SQLite is held in reserve for the web port (M15). Re-evaluate if the upstream PR path closes and the fork proves too expensive to maintain.
