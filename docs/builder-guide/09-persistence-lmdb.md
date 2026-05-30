# 09 — Persistence (LMDB) + watermarks

**Status: LANDED** · audience: agents · prereqs: [08](08-eventstore.md)

> **Read this first.** The durable LMDB backend is **not implemented
> yet**. `LmdbEventStore` is a skeleton: every `EventStore` trait method
> returns `StoreError::Io("lmdb-backend feature not enabled")`
> *unconditionally* — even when compiled `--features lmdb-backend`. Only
> `open()` has a feature-gated branch, and it merely `mkdir`s
> `path/nostr/` + `path/nmp/`
> (`crates/nmp-core/src/store/lmdb.rs:34-60`, all impl methods
> `:62-226`). The on-disk schema, key encoding, watermark semantics, and
> trait contract are **settled in design + ADRs**; the implementation is
> **M3 phase 2** (`docs/plan/m3-persistence.md`). What ships today is
> `MemEventStore` (see [08](08-eventstore.md)).

This section is the contract an agent needs to land M3 phase 2: the byte
layout, the watermark row, what must survive a restart, and the build
matrix. It distills the design docs — it does not restate them.

---

## Backend abstraction

`enum StorageBackend { Memory, Lmdb { path } }` and the factory
`open_event_store(&StorageBackend) -> Result<Box<dyn EventStore>,
StoreError>` (`crates/nmp-core/src/store/mod.rs:36-51`). The actor holds
`Box<dyn EventStore>`; backends are substitutable. `MemEventStore` is
always compiled (tests + pre-M15 web); `LmdbEventStore` is always
compiled but only *functional* with `--features lmdb-backend` once the
implementation lands (`store/mod.rs:10-15`).

`DomainHandle` already carries an `#[cfg(feature = "lmdb-backend")]`
`Lmdb` variant whose every method returns
`"lmdb-backend not yet implemented"`
(`store/events.rs:37-41` for the variant; `:67-123` for the stubbed
`put`/`get`/`delete`/`scan_prefix`) — the seam exists; the body does not.

**ADR-0011** (`docs/decisions/0011-lmdb-env-sharing.md`): NMP owns the
`lmdb::Environment` and injects it into `nostr-lmdb`
(`NostrLMDB::with_env`, upstream PR or pinned fork). Rationale: every
`insert` must commit event + provenance + all NMP secondaries in **one
`RwTxn`** — two independent `Environment` handles cannot roll back each
other, and a partial write would surface as a corrupt `AppUpdate`
(D6 violation, ADR-0011 §Context). Layout: `path/nostr/` owned by
`nostr-lmdb`, `path/nmp/` owned by NMP (watermarks, provenance, claims,
domain rows, tombstones — `lmdb.rs:22-27`).

---

## Key-encoding table (composite → bytes)

All integers **big-endian** so LMDB's bytewise comparator matches
numeric order; `created_at_desc_be = (u64::MAX -
created_at).to_be_bytes()` gives newest-first forward scans without
`MDB_PREV` (`docs/design/lmdb/keys.md:31-35`).

| Sub-db | Owner | Key | Value |
|---|---|---|---|
| (events + filter idx + del) | nostr-lmdb | upstream | upstream |
| `idx_author_kind` | NMP | `pubkey[32] ‖ kind_be[4] ‖ ts_desc[8] ‖ id[32]` | ∅ |
| `idx_kind_dtag` | NMP | `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag` | `id[32]` |
| `idx_kind_dtag_time` | NMP | `kind_be[4] ‖ dtag_len_be[2] ‖ dtag ‖ ts_desc[8] ‖ id[32]` | ∅ |
| `idx_etag_time` | NMP | `etarget[32] ‖ ts_desc[8] ‖ id[32]` | `kind_be[4]` |
| `idx_ptag_time` | NMP | `ptarget[32] ‖ ts_desc[8] ‖ id[32]` | `kind_be[4]` |
| `idx_kind_time` | NMP | `kind_be[4] ‖ ts_desc[8] ‖ id[32]` | ∅ |
| `idx_expires` | NMP | `expires_at_be[8] ‖ id[32]` | ∅ |
| `tombstones` | NMP | `target_id[32]` | CBOR `TombstoneRow` |
| `tombstones_addr` | NMP | `pubkey[32] ‖ kind_be[4] ‖ dtag_len_be[2] ‖ dtag` | CBOR `TombstoneRow` |
| `provenance` | NMP | `id[32]` | CBOR `ProvenanceRow` |
| `watermarks` | NMP | `filter_hash[32] ‖ relay_url_bytes` | CBOR `WatermarkRow` |
| `idx_watermark_relay` | NMP | `relay_url_bytes ‖ filter_hash[32]` | ∅ |
| `claims_meta` | NMP | `claimer_id_be[8]` | CBOR `BTreeSet<EventId>` |
| `domain_<ns>_data` | NMP/module | module-defined | module-defined |
| `domain_<ns>_idx_<n>` | NMP/module | `index_key ‖ primary_key` | ∅ |
| `_meta` | NMP | namespace string | CBOR `{schema_version, opened_with}` |

Source: `docs/design/lmdb-schema.md:77-94`,
`docs/design/lmdb/keys.md:9-27`. The d-tag length prefix prevents
`d="foo"` vs `d="foob"` prefix-scan aliasing
(`docs/design/lmdb/keys.md:53-55`). Worked insert example (5 page
writes for a kind:1; 1 read + 1 write for a duplicate):
`docs/design/lmdb/keys.md:164-194`.

---

## Watermarks row spec

The store keeps a durable per-`(filter_hash, relay)` bookmark. The type
**already ships** (used by `MemEventStore`):
`WatermarkRow` / `WatermarkKey` / `SyncMethod` / `Coverage` —
`crates/nmp-core/src/store/types/mod.rs:18` →
`crates/nmp-core/src/store/types/watermark.rs:10-43`.

| Field | Type | Meaning |
|---|---|---|
| `key.filter_hash` | `[u8;32]` | BLAKE3 of canonicalised filter |
| `key.relay_url` | `RelayUrl` | exact-key suffix |
| `synced_up_to` | `u64` | unix s — "complete on this relay up to T" |
| `last_sync_method` | `SyncMethod` | `Negentropy`/`ReqScan`/`Manual` |
| `last_negentropy_state` | `Option<Vec<u8>>` | engine-opaque resume blob (M4) |
| `bytes_saved_vs_req` | `u64` | cumulative; diagnostics |
| `updated_at` | `u64` | unix s |

`coverage(key) -> Coverage` classifies freshness
(`store/types/watermark.rs:35-43`,
`docs/design/lmdb/watermarks.md:32-44`):

- `CompleteAsOf(t)` — `synced_up_to ≥ now - coverage_staleness_secs`
  (default 300 s). A cache miss here is **authoritative**: "does not
  exist on that relay."
- `PartialUpTo(t)` — stale row → fetch needed.
- `Unknown` — no row → always fetch.

`filter_hash` canonicalisation (sort tag arrays/kinds/authors/ids,
fixed CBOR field order, BLAKE3): `docs/design/lmdb/watermarks.md:85-98`.
A filter with `limit` hashes differently from one without — coverage
semantics genuinely differ; sharing across limits is a *planner-side*
strip, not a store-side one (`watermarks.md:98`).

---

## Survives restart

When the LMDB backend lands (M3 phase 2), the following are durable
across app kill + relaunch — none rebuilt from network:

- **Events**: primary rows + all NMP secondaries, committed in one
  `RwTxn` per `insert` (ADR-0011 §Decision).
- **Watermarks**: re-read on `open()` into a hot
  `HashMap<WatermarkKey, WatermarkRow>`; every `write_watermark` updates
  map + LMDB row in one txn (`docs/design/lmdb/watermarks.md:47-50`).
  This is what makes "authoritative cache-miss" survive restart.
- **Tombstones**: `tombstones` + `tombstones_addr` outlive deleted
  events — a redelivered deleted event is re-suppressed
  (`docs/design/lmdb-schema.md:152-156`).
- **Provenance**: per-event sidecar; deterministic `primary` relay
  recomputed by sort (`docs/design/lmdb/watermarks.md:52-79`).
- **Claims**: `claims_meta` persists the per-`ClaimerId` pin set so a
  mid-shutdown crash doesn't lose hot-set protection; on cold start
  claims are **re-derived from re-opened views** and stale rows dropped
  (`docs/design/lmdb/gc.md:198`).
- **App-module domain rows + schema versions**: per-namespace sub-dbs
  (namespaces declared by each module crate, e.g. `"fixture.todo.domain"`);
  migrations run at `open()`, data write + `_meta` version bump in **one
  txn** (no TOCTOU) (`docs/design/lmdb/watermarks.md:130-165`).

Exit gate: cold-start time-to-first-painted-timeline ≤ 1.5 s on iPhone
12 with primed LMDB; ≤ 100 MB working set at 100 views / 10k hot / 1M on
disk (`docs/plan/m3-persistence.md:16-22`, `docs/design/lmdb/gc.md:200-219`).

---

## `lmdb-backend` feature build matrix

| Build | `LmdbEventStore::open()` | trait methods | Use |
|---|---|---|---|
| default (no feature) | `Err(Io "recompile with --features lmdb-backend")` | `Err` (skeleton) | `Memory` backend only |
| `--features lmdb-backend` *(today)* | `Ok` after `mkdir nostr/ nmp/` — **no env opened** | `Err(Io "feature not enabled")` *(still — not implemented)* | not usable |
| `--features lmdb-backend` *(post-M3 phase 2)* | opens shared `lmdb::Environment` | functional | iOS/Android/Desktop production |

Today, even with the feature on, the store is non-functional past
`open()` (`store/lmdb.rs:57-59` `not_enabled()` is returned by every
method irrespective of the cfg gate). Selecting `StorageBackend::Memory`
is the only working path until M3 phase 2 lands.

---

## Anti-patterns

1. **App-side persistence parallel to EventStore.** SwiftData/Room
   shadow copies re-introduce every staleness bug §7.1 prevents and
   violate D4. The store is the single durable writer; the platform
   keeps no event state.
2. **Cross-process LMDB sharing.** One `Environment` per app data
   directory, single process. LMDB cross-process downgrade is
   unsupported (`docs/design/lmdb/watermarks.md:169`); a second process
   touching the env corrupts MVCC assumptions.
3. **Sharing an `lmdb::Env` from another crate.** ADR-0011 violation:
   NMP must *own* the env and inject it into `nostr-lmdb`; an env handed
   in from elsewhere breaks single-`RwTxn` atomicity and thus D6.
4. **Treating the cache as source of truth.** A non-empty result is
   never proof a query is complete; only `Coverage::CompleteAsOf` over a
   covering watermark makes absence authoritative (D4, see
   [08](08-eventstore.md)).
5. **Assuming `--features lmdb-backend` gives you durability today.** It
   does not — it only creates two empty directories. Build against
   `MemEventStore` and the design docs until M3 phase 2.

---

See also: [08 — EventStore + insert invariants + GC](08-eventstore.md) · [13 — Sync engine — `nmp-nip77`](13-sync-engine.md) · [27 — Doc/code discrepancies](27-discrepancies.md)
