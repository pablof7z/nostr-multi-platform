# 09 — Persistence (LMDB) + coverage ledger

**Status: SHIPS, feature-gated** · audience: agents · prereqs: [08](08-eventstore.md)

> **Read this first.** The durable LMDB backend is implemented in `nmp-store`.
> Default builds still use `MemEventStore`; `LmdbEventStore::open(path)` returns
> an explicit "recompile with --features lmdb-backend" error unless the feature
> is enabled. With `--features lmdb-backend`, `LmdbEventStore` opens the shared
> LMDB environment, the upstream Nostr sub-dbs, and the NMP-owned sub-dbs used by
> provenance, tombstones, domain rows, relay indexes, LRU/expiry GC, relay scores,
> and the K3 coverage ledger.

This section is the contract an agent needs when wiring a production app to
durable storage. Older pre-implementation notes no longer apply; storage now
lives in `crates/nmp-store/`, re-exported through `nmp_core::store::*`.

---

## Backend abstraction

`enum StorageBackend { Memory, Lmdb { path } }` and
`open_event_store(&StorageBackend) -> Result<Box<dyn EventStore>, StoreError>`
live in `crates/nmp-store/src/lib.rs`. The actor holds `Box<dyn EventStore>`;
backends are substitutable behind the same trait.

| Backend | Build mode | Behavior |
|---|---|---|
| `MemEventStore` | always compiled | In-memory test/web/default path; no restart durability. |
| `LmdbEventStore` | feature off | Type exists, but `open()` and trait methods return `lmdb-backend feature not enabled`. |
| `LmdbEventStore` | `--features lmdb-backend` | Opens real LMDB env via `Lmdb::open_env` + `Lmdb::with_env`; all `EventStore` methods delegate to LMDB subsystems. |

The feature-gated implementation is split by subsystem:

- `lmdb/open.rs` opens the shared environment and all NMP sub-dbs.
- `lmdb/store_impl.rs` implements `EventStore` by delegating to query, insert,
  delete, domain, GC, relay-index, relay-score, and coverage modules.
- `lmdb/coverage.rs` owns the coverage-ledger row reads/writes.

---

## Shared-env rule

**ADR-0011** (`docs/decisions/0011-lmdb-env-sharing.md`) still owns the durable
storage invariant: NMP owns one LMDB environment and injects it into the
Nostr-LMDB layer (`Lmdb::with_env`). Event insert plus NMP secondaries must
commit in one write transaction; two independent environments cannot roll back
as a unit and would violate D4/D6.

The current open path creates one env under the configured app data directory,
then opens upstream Nostr databases and these NMP databases on that env:

| Sub-db | Purpose |
|---|---|
| `nmp-provenance` | relays that delivered an event and when |
| `nmp-tombstones` | id-delete tombstones |
| `nmp-addr-tombstones` | addressable delete tombstones |
| `nmp-domain-versions` | module/domain schema versions and one-time backfill markers |
| `nmp-domain-data` | module/domain rows |
| relay-author scores | LMDB-specific relay-author-score store |
| `nmp-lru-access` | Phase-2 LRU access sequence for GC |
| `nmp-expiry-index` | expiration-time index |
| `nmp-relay-index` | relay URL -> event ids reverse lookup |
| `nmp-coverage` | K3 coverage ledger |

All integer keys remain big-endian so LMDB byte ordering matches numeric order.
NMP-owned composite keys use explicit separators or length prefixes when a
variable-length segment can otherwise alias a longer value.

---

## Coverage ledger

The durable read-path fact is `CoverageRow` in
`crates/nmp-store/src/types/coverage.rs`:

| Field | Meaning |
|---|---|
| `filter_hash` | canonical logical-filter hash, with the `sub-` prefix stripped |
| `relay` | exact relay URL |
| `covered_through` | downward-closed timestamp: this relay is known complete for `[0, T]` |

The LMDB key is `filter_hash || 0x1F || relay`; the value is
`covered_through` as big-endian `u64`. The `EventStore` trait exposes:

- `record_coverage(filter_hash, relay, covered_through)`
- `get_coverage(filter_hash, relay)`
- `coverage_max_for_filter_hash(filter_hash)`
- `coverage_rows_for_filter_hash(filter_hash)`
- `gc_step_with_pins_and_coverage(...)`

Presence is not coverage. A stored event at time `300` does not prove the store
has everything before `300`; only a completed EOSE/NEG-DONE coverage row can
claim the downward-closed range. This is why the read path uses the coverage
ledger for since-floor decisions when the coverage-ledger flag is enabled.

---

## Survives restart

With `--features lmdb-backend`, these facts are durable across app kill and
relaunch:

- **Events and upstream indexes.** Upstream Nostr-LMDB owns event encoding and
  primary/filter indexes.
- **NMP secondaries.** Provenance, tombstones, relay reverse index, expiry
  index, and LRU access sequence are stored in NMP sub-dbs.
- **Coverage ledger.** Completed coverage rows survive restart and remain
  monotonic unless an explicit finite durable-retention policy removes covered
  events; the GC backstop lowers affected coverage rows in the same
  lock/transaction as the delete.
- **Domain rows.** Module/domain rows and schema versions survive restart.
  Migrations run through `EventStore::run_migrations`.

Production GC does not use the LRU access sequence to delete otherwise-valid
events by default. The sequence is durable so an explicit disk/user retention
policy can make deterministic LRU decisions when such a policy is configured.

`MemEventStore` implements the same trait shape for tests and default builds,
but it is not durable.

---

## `lmdb-backend` feature build matrix

| Build | `LmdbEventStore::open()` | trait methods | Use |
|---|---|---|---|
| default, no feature | `Err(Io "lmdb-backend feature not enabled ...")` | feature-off stub returns `Err` | Memory backend only |
| `--features lmdb-backend` | opens/creates shared LMDB env and NMP sub-dbs | functional LMDB implementation | native durable storage |

There is no silent fallback from failed LMDB open to memory. A durable app must
surface the open failure as state/diagnostics rather than accidentally running
against an in-memory store.

---

## Anti-patterns

1. **App-side persistence parallel to EventStore.** SwiftData/Room shadow copies
   re-introduce staleness bugs and violate D4. The store is the single durable
   writer; platform code keeps no event state.
2. **Cross-process LMDB sharing.** Use one environment per app data directory in
   one process. A second process touching the same env breaks the assumptions the
   store and GC make.
3. **Sharing an `lmdb::Env` from another crate.** ADR-0011 requires NMP to own
   the env and inject it into Nostr-LMDB so event and NMP secondary writes are
   atomic together.
4. **Treating presence as coverage.** A local event is not proof of a complete
   historical range. Since-floor and authoritative-miss decisions must use the
   coverage ledger, not "oldest event seen".
5. **Assuming the feature is on because the type exists.** `LmdbEventStore` is
   compiled in all builds, but only functional with `--features lmdb-backend`.

---

See also: [08 — EventStore + insert invariants + GC](08-eventstore.md) ·
[13 — Sync engine — `nmp-nip77`](13-sync-engine.md) ·
GitHub Issues or the owning doc
