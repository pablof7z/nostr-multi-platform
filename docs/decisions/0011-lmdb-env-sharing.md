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

**Why this matters (D6):** if the store returns wrong data because of a partial write, that wrong data eventually reaches host-facing update frames. There is no way to label one projection inside an already-emitted snapshot as partially corrupt. The only safe design is to prevent partial writes at the store layer.

## Decision

**NMP owns the `lmdb::Environment` and injects it into `nostr-lmdb`.**

Concretely:

1. `LmdbEventStore::open(path)` calls `lmdb::Environment::open(path, ...)` and receives the sole `Environment` handle.
2. This `Environment` is passed to the forked `nostr-lmdb` via `Lmdb::with_env(env: Arc<lmdb::Environment>)`.
3. Both the fork's sub-databases and NMP's sub-databases are opened under this single `Environment`.
4. `insert()` opens one `lmdb::RwTxn`, saves the event plus NMP's secondary writes in that txn, then commits once. Either all writes land or none do.
5. Migration steps open one `RwTxn` per step and commit the data writes and the `_meta` version bump together (see `lmdb/watermarks.md` §4.2).

The shipped surface lives in `crates/nmp-store/src/lmdb/` (`open.rs` calls `Lmdb::with_env`); the env-injection + txn-scoped write primitive ship in the `nmp-nostr-lmdb` fork rather than as an upstream change.

## Consequences

**Positive:**
- Single-commit atomicity for all `insert()` writes. No half-state visible to any reader.
- Simpler recovery: LMDB's built-in crash-recovery (WAL + mmap) is sufficient; no NMP-side recovery logic needed.
- Migration steps are atomic by construction (§4.2 of `lmdb/watermarks.md`).

**Negative:**
- Requires a forked `nostr-lmdb` (`nmp-nostr-lmdb`) carrying the env-injection constructor and txn-scoped write primitive. The fork is a maintenance surface.
- NMP becomes the LMDB environment owner, which means it is responsible for tuning `mapsize`, `max_dbs`, and `max_readers`. These were previously handled by `nostr-lmdb` internally. The upstream defaults are well-chosen for Nostr workloads; NMP adopts them as its starting point and adjusts only when benchmarks show a regression.

## Alternatives considered

**A. Let `nostr-lmdb` own the environment; use a two-phase write with WAL-recovery.** Rejected because of the added complexity, the recovery-window ambiguity, and the write-amplification cost — two independent `Environment` handles cannot share a single commit, so no partial-write rollback is possible.

**B. Replace `nostr-lmdb` with a hand-rolled LMDB layer.** Rejected in the master doc §1 (see "Rejected alternatives"). Reinvents 2 000+ LOC of battle-tested NIP-09 / replaceable event logic at high bug risk. Not justified.

**C. Use SQLite (or another store) that supports multi-table transactions natively.** SQLite supports cross-table ACID transactions without environment sharing issues. Rejected because (a) the iOS-disk-WAL fsync cost at our 10k-event hot working set is higher than LMDB's mmap model, (b) `nostr-lmdb` gives us NIP-77 negentropy integration we would have to re-implement, (c) SQLite remains a candidate for the post-v1 web port. Re-evaluate if the upstream PR path closes and the fork proves too expensive to maintain.
