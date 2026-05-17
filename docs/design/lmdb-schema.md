# Design: LMDB schema + EventStore trait + GC policy (M3)

> **Audience:** kernel implementers landing M3 (persistence).
> **Status:** rev 0 — proposed; opens ADR slot for any open-question resolution.
> **Companion docs:** [`lmdb/trait.md`](lmdb/trait.md), [`lmdb/keys.md`](lmdb/keys.md), [`lmdb/gc.md`](lmdb/gc.md), [`lmdb/watermarks.md`](lmdb/watermarks.md), [`lmdb/tests.md`](lmdb/tests.md).
> **Prerequisites:** `docs/product-spec/subsystems.md` §7.1 (insert invariants), `docs/decisions/0003-working-set-memory.md` (GC policy intent), `docs/decisions/0009-app-extension-kernel-boundary.md` (DomainModule storage), `docs/design/kernel-substrate.md` §2 (DomainModule trait).
> **Plan reference:** [`docs/plan.md`](../plan.md) §M3.

---

## 1. Decision: wrap `nostr-lmdb` behind our own `EventStore` trait

**Adopt `nostr-lmdb` as the on-disk byte store. Wrap it behind the NMP `EventStore` trait. Add NMP-owned LMDB sub-databases for the rows `nostr-lmdb` does not model.**

The competing options were (1) use `nostr-lmdb` directly via its concrete `NostrLMDB` type (or via `nostr_database::NostrEventsDatabase`), (2) wrap behind our own trait, or (3) hand-roll an LMDB layer.

**`nostr-lmdb` gives us** (per `docs.rs/nostr-lmdb`): `save_event(&Event)`, `event_by_id(&EventId)`, `check_id(&EventId) -> DatabaseEventStatus`, `query(Filter) -> Events`, `count(Filter)`, `delete(Filter)`, `wipe()`, `negentropy_items(Filter) -> Vec<(EventId, Timestamp)>`. It owns the page allocator, the LMDB environment, primary by-id store, indexes derived from `Filter`, replaceable / parameterized-replaceable supersession, and NIP-09 delete handling. It is the only mature Rust LMDB store for Nostr events with proven NIP-77 integration; reinventing it is a year of work that we will not recoup.

**What `nostr-lmdb` does *not* model** (the gap that justifies a wrapper):

| Concern | Why `nostr-lmdb` doesn't cover it | Where NMP needs it |
|---|---|---|
| Per-relay provenance (which relays delivered each event; first seen / last seen) | Out of scope; the crate models events, not their wire history | `subsystems.md` §7.1 "Duplicate id → merge relay provenance set"; ADR-0007 diagnostics; outbox routing scoring in M2+ |
| Sync watermarks `(filter_hash, relay) → synced_up_to` | Out of scope; the crate does not know about logical filters or relay identity | `subsystems.md` §7.1 + §7.8; M4 NIP-77 engine needs them to be authoritative |
| Claim register / release for view-driven GC | Out of scope; the crate has no concept of an "open view" | ADR-0003; current in-memory analogue in `kernel/mod.rs:315` `profile_claims: HashMap<String, BTreeSet<String>>` |
| Working-set hot/cold split with eviction LRU | LMDB is OS-paged; the crate trusts the kernel page cache | ADR-0003 numeric gate (≤ 100 MB at 100 views / 10k hot) |
| Kernel-side secondary indexes for query shapes our planner uses (e.g. `(p-tag, timestamp)` desc scan, `(e-tag, timestamp)` desc scan, `(expires_at, event_id)` for NIP-40 wakeups) | The `Filter` API recomputes per call; not optimal for our planner's repeat shapes | Planner cache-coverage queries (§7.2); NIP-40 expiration scheduling (§7.1) |
| Tombstone-as-row that survives independent of the deleted event | The crate suppresses re-insert via its own delete index; we want it exposed for export / restoring across re-syncs | `subsystems.md` §7.1 kind:5 row "persisted as tombstone so later re-insertion is suppressed" |
| `DomainModule` rows (drafts, settings, action ledger, projection caches) | Entirely out of scope; the crate stores Nostr `Event` only | ADR-0009 + `kernel-substrate.md` §2 — kernel hosts non-Nostr typed rows |
| Migrations versioned per namespace | Out of scope | `kernel-substrate.md` §2: `DomainModule::migrations() -> Vec<DomainMigration>` |
| `nmp dump` deterministic export | Out of scope | M3 exit criteria; M11 cross-app proof |

**Therefore.** `EventStore` is a NMP-owned trait, with one production impl `LmdbEventStore` that holds (a) a `NostrLMDB` for the canonical event store and Nostr-shaped queries, and (b) NMP-owned secondary LMDB sub-databases under the same `lmdb::Environment` for the gap rows. The in-memory backend (`MemEventStore`) remains, both for tests and as the web-pre-M15 fallback. See [`lmdb/trait.md`](lmdb/trait.md) for the exact trait shape and the relayed-vs-owned method split.

**Rejected alternatives.**

- *Use `NostrLMDB` directly, no wrapper.* Loses every gap row above. Forces the kernel actor to know about LMDB transactions and a non-NMP concrete type, breaking the `Box<dyn EventStore>` substitutability M3 requires.
- *Roll our own.* Reinvents NIP-09 / replaceable handling that `nostr-lmdb` already gets right. ~2,000 LOC of avoidable code with a worse bug surface than upstream.
- *SQLite-backed `nostr-sdk` store.* Larger memory footprint at our 10k-event hot working set; iOS-disk-WAL fsync cost not justified for this access pattern. Held in reserve for the web port (M15) if IndexedDB OPFS proves unworkable.

## 2. Subsystem ownership map

```
crates/nmp-core/src/store/
  mod.rs                — trait re-exports + factory
  trait.rs              — `EventStore` (see lmdb/trait.md)
  mem.rs                — in-memory backend (preserved from kernel/mod.rs current state)
  lmdb/
    mod.rs              — `LmdbEventStore` orchestrator
    env.rs              — `lmdb::Environment` + sub-db handles + open()
    events.rs           — wraps `nostr_lmdb::NostrLMDB`; primary-by-id, replaceable rules, kind:5 handling
    secondary.rs        — NMP-owned secondary indexes (see lmdb/keys.md §3)
    provenance.rs       — provenance sub-db (see lmdb/watermarks.md §2)
    watermarks.rs       — watermark sub-db (see lmdb/watermarks.md §1)
    claims.rs           — claim register / release + hot-set hints (see lmdb/gc.md §2)
    gc.rs               — `gc_step()` algorithm (see lmdb/gc.md §3)
    domain.rs           — per-DomainModule sub-db namespacing + migration runner
    dump.rs             — `nmp dump` (see §9 below)
```

Each file is bounded ≤ 300 LOC per AGENTS.md. The trait module is read by the actor; backend modules are read only by the orchestrator.

## 3. EventStore trait

See [`lmdb/trait.md`](lmdb/trait.md) for the exact `pub trait EventStore` signature with all required methods, return types, and the `StoreError` enum. Summary:

- **Reads:** `get_by_id`, `scan_by_author_kind`, `scan_by_kind_dtag`, `scan_by_etag`, `scan_by_ptag`, `scan_by_kind_time`, `scan_expiring_before`. All `scan_*` methods return a streaming `EventIter` so the planner pages without materialising. Cache-coverage queries take a `WatermarkKey` and answer authoritatively.
- **Writes:** `insert(event, RelayUrl)` returns `InsertOutcome` matching §7.1's table. `delete_by_filter` for foreign-relay cleanups. `tombstones_for` for replay.
- **Watermarks / sync:** `read_watermark`, `write_watermark`, `list_watermarks_for_relay`.
- **GC:** `claim(ClaimerId, &[EventId])`, `release(ClaimerId)`, `hot_set_hint(&[EventId])`, `gc_step(GcBudget) -> GcReport`.
- **Domain rows:** `domain_open(namespace) -> DomainHandle` returns a typed handle; `DomainHandle::put/get/scan_index` is the per-DomainModule API.
- **Migration:** `run_migrations(&[DomainMigration])` runs at startup, transactional per migration.
- **Export:** `dump(out: &mut dyn Write, format: DumpFormat) -> Result<DumpStats>`.

**Error semantics.** All methods return `Result<T, StoreError>`. Per doctrine D3, store errors do **not** cross FFI — the actor maps every variant to either (a) a typed `Effect` (e.g. `StoreCorrupt`, surfaces via diagnostics + toast), (b) a `tracing::warn!` log + degraded state, or (c) a panic at startup if the LMDB environment refuses to open. The trait itself uses `Result` since it is internal to the actor process.

## 4. Key encoding

Full byte layout for primary + every secondary in [`lmdb/keys.md`](lmdb/keys.md). At a glance:

- Primary `events`: `event_id[32]` → `Event` (CBOR via `nostr` crate's serialization). Owned by `nostr-lmdb`.
- Secondary `idx_author_kind`: `pubkey[32] || kind_be[4] || created_at_be[8] || event_id[32]` → empty. NMP-owned.
- Secondary `idx_kind_dtag`: `kind_be[4] || dtag_len_be[2] || dtag_bytes || pubkey[32]` → `event_id[32]`. NMP-owned. Parameterized replaceable address lookup.
- Secondary `idx_etag_time`, `idx_ptag_time`: `tag_value[32] || created_at_desc_be[8] || event_id[32]` → empty. NMP-owned. `created_at_desc = u64::MAX - created_at` so a forward LMDB scan is newest-first.
- Secondary `idx_kind_time`: `kind_be[4] || created_at_desc_be[8] || event_id[32]` → empty.
- Secondary `idx_expires`: `expires_at_be[8] || event_id[32]` → empty. Scanned by the NIP-40 reaper.
- `tombstones`: `target_id[32]` → `TombstoneRow { kind5_event_id, deleter_pubkey, deleted_at, sources: Vec<RelayUrl> }` (CBOR).

`created_at_be` is big-endian so byte order matches numeric order; `created_at_desc_be = u64::MAX - created_at` then big-endian for newest-first scans without `MDB_LAST + MDB_PREV`.

All secondaries are maintained inside the same `RwTxn` as the primary write — atomicity is achieved by LMDB transactionality, not by post-hoc reconciliation.

## 5. Watermark table

See [`lmdb/watermarks.md`](lmdb/watermarks.md) for full layout. Row shape (CBOR):

```rust
struct WatermarkRow {
  filter_hash: [u8; 32],     // BLAKE3 of canonicalised filter (see watermarks.md §3)
  relay_url: String,
  synced_up_to: u64,         // unix seconds
  last_sync_method: SyncMethod, // Negentropy | ReqScan | Manual
  last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob
  bytes_saved_vs_req: u64,
  updated_at: u64,
}
```

Key: `filter_hash[32] || relay_url_bytes` (no length prefix needed — relay URL is the variable suffix; lookup uses exact key). Populated by M4 (NIP-77) and consulted by M2's planner (cache-coverage check before issuing backfill REQ). Survives restarts; loaded into the actor on startup as a `HashMap<(filter_hash, relay_url), WatermarkRow>` for hot lookups, with all writes going through `EventStore` for durability.

## 6. Migration plumbing

See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4. A `DomainModule` (per `kernel-substrate.md` §2) declares `const NAMESPACE: &'static str` and `const SCHEMA_VERSION: u32` plus `fn migrations() -> Vec<DomainMigration>`. The store assigns one LMDB sub-database per `(namespace, "data")`, plus one per `(namespace, index_name)` for each declared index. A `_meta` sub-database tracks `(namespace, current_version)`.

The current `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:41`) discards the concrete `M: DomainModule` type after `register_domain::<M>()` returns — only the `ModuleDescriptor` is retained. The store cannot get from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()` at runtime. M3 adds a `DomainFactories { schema_version: fn() -> u32, migrations: fn() -> Vec<DomainMigration>, indexes: fn() -> Vec<DomainIndex> }` struct attached per descriptor, populated by capturing the `M::*` consts and fns in `fn`-pointer closures at register time. This matches the existing `key_fn: fn(&[u8]) -> Option<Vec<u8>>` pattern in `DomainIndex` (`crates/nmp-core/src/substrate/domain.rs:18`) — no `Box<dyn DomainModule>` and no new trait object-safety constraints on `DomainModule`. The change is additive to the substrate module surface. See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4.1 for the registry-side code shape.

On startup:

1. For every registered `DomainModule`, read its row from `_meta`.
2. If absent, treat current as 0 and run all migrations from 0 to `SCHEMA_VERSION` in one `RwTxn` per step.
3. If present and less than `SCHEMA_VERSION`, run the missing steps.
4. If greater, refuse to start (downgrade not supported); surface as `Effect::DomainSchemaTooNew { namespace }`.

Each `DomainMigration::apply` receives a `MigrationTx` with put/get/delete + index rebuild helpers. Rollback semantics: each migration step is its own LMDB write transaction; failure aborts the step cleanly. If migration N succeeds and N+1 fails, the store stays at version N — the actor refuses to start the affected module and the rest of the kernel runs in degraded mode (the module's actions return `ActionRejection::ModuleUnavailable`).

## 7. GC working-set policy

See [`lmdb/gc.md`](lmdb/gc.md) for the eviction algorithm. Formal statement (matches ADR-0003):

```
hot_resident = {e | e is in claim_pinned}
             ∪ {e | e is in open_view_cover}
             ∪ {e | e is among the ≤10k most-recently-touched events}

cold = stored_events \ hot_resident
```

`hot_resident` lives in a `lru::LruCache<EventId, Arc<Event>>` capped at the configured hot ceiling (default 10,000) plus an unbounded pinned overlay holding events with non-zero claim count. `cold` lives only on disk; lookup pays one LMDB `get` (memory-mapped — typically already in OS page cache for recently-evicted items).

**Eviction algorithm.** On any insert that pushes the LRU over its ceiling, the oldest non-pinned entry is dropped. `gc_step()` is called periodically by the actor (default every 60 s and on memory pressure callbacks from `MemoryWarningCapability`): it (a) reaps NIP-40 expired events using `idx_expires`, (b) trims the LRU to `target_hot_size`, (c) deletes tombstones older than `tombstone_retention` (default 90 days) whose target event is absent from the store, (d) returns a `GcReport` for diagnostics.

ADR-0003's numbers are preserved as the M3 exit gate (§11 below): ≤ 100 MB working-set at 100 active views / 10k hot events / 1M cached on disk.

## 8. Replaceable + tombstone semantics

The `insert()` path implements exactly the §7.1 invariants:

- **Replaceable (kinds 0, 3, 10000–19999).** Look up the existing event for `(pubkey, kind)` in `idx_author_kind` (most recent suffix). If incoming `created_at` is newer, replace; if equal, keep lexicographically smallest `id`; else drop. Replacement deletes the old primary row and all secondary entries in the same `RwTxn`.
- **Parameterized replaceable (30000–39999).** Same algorithm keyed on `(pubkey, kind, d-tag)` via `idx_kind_dtag` (which holds `event_id` as value so we don't need a separate `idx_author_kind_dtag`; the dtag prefix is unique per author by Nostr semantics — see [`lmdb/keys.md`](lmdb/keys.md) §3.2 for the per-author scoping note).
- **Kind:5 self-delete.** Verify signature, scan referenced `e` and `a` tags, for each target `e_id` that is authored by the deleter or whose `a` address matches `(deleter_pubkey, kind, d-tag)`: delete the primary + all secondaries + write the tombstone row. Tombstone timestamp = `max(existing.deleted_at, kind5.created_at)`. Re-insert of the deleted event id is suppressed at insert time by a `tombstones.contains(event_id)` check.
- **Foreign kind:5.** A kind:5 referencing events not authored by the kind:5's `pubkey` is ignored (per spec) — the event is *still stored* as a kind:5 (so other clients can render it / dedup it), but it has no side effect on the targets. The tombstone row is **not** written.
- **NIP-40 expiration.** On insert, parse `expiration` tag; if present, write `idx_expires`. On `gc_step()`, scan `idx_expires` for keys with `expires_at_be ≤ now`, delete them like kind:5 (full primary + secondaries + tombstone marker noting `kind: Expired`).

The tombstone schema is in [`lmdb/keys.md`](lmdb/keys.md) §4.

## 9. Provenance: per-row sidecar sub-database

**Decision: separate `provenance` sub-database keyed by `event_id[32]`.** Value is CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }` where `ProvenanceEntry = { relay_url, first_seen_ms, last_seen_ms, primary: bool }`.

Rejected: stuffing provenance into the `Event` row. That requires re-serializing the full `Event` on every relay redelivery (high write amplification — popular events arrive 5–20× from the relay fan-out) and forks the `nostr-lmdb` row format, which we explicitly want to keep upstream-compatible. The sidecar is appended cheaply with a single CBOR re-encode of the (typically small) `sources` vector.

On duplicate-id insert (§7.1 row 2), `insert()` does not touch the primary; it only updates the provenance sidecar (`last_seen_ms` bump on the matching `ProvenanceEntry`, or append). The "primary relay" — for outbox-routing scoring (M2) and ADR-0007 diagnostics — is deterministically the first relay observed (`sources[0]` after sort by `first_seen_ms`).

The export format (§ next) includes the provenance row alongside each event so a `nmp dump` round-trip restores it.

## 10. Backup / export format

`nmp dump` writes line-delimited JSON to stdout (or a file). Each line is a single tagged record:

```json
{"type":"event","event": {...nostr event...},"provenance":[{"relay_url":"wss://relay.primal.net","first_seen_ms":1747000000000,"last_seen_ms":1747001234567,"primary":true}]}
{"type":"tombstone","target_id":"abc...","kind5_event_id":"def...","deleter_pubkey":"...","deleted_at":1747000000,"sources":["wss://..."]}
{"type":"watermark","filter_hash":"hex32","relay_url":"wss://...","synced_up_to":1747000000,"last_sync_method":"Negentropy","bytes_saved_vs_req":12345,"updated_at":1747000123}
{"type":"domain","namespace":"twitter.drafts","schema_version":1,"key_hex":"...","value_b64":"..."}
```

JSONL is the chosen format because (a) it streams (no holding the full dump in memory; cold-events page in as scanned), (b) it diffs cleanly (one record per line), (c) any line is independently parsable for partial recovery, (d) `jq` works out of the box. Binary CBOR is faster but loses ad-hoc inspectability — JSONL is the right tradeoff for an export format.

`nmp restore` is symmetric: read JSONL, replay through `insert()` for events (so all secondaries are rebuilt from scratch — provenance is restored separately by writing the sidecar row directly after each event), `write_watermark` for watermarks, `DomainHandle::put` for domain rows. Restore is idempotent: replaying the same dump twice produces the same store.

## 11. Test plan

See [`lmdb/tests.md`](lmdb/tests.md) for the full mapping of every spec §7.1 invariant to a concrete test file under `crates/nmp-testing/tests/`. Highlights:

| Invariant (§7.1) | Test file |
|---|---|
| Insert API single path | `crates/nmp-testing/tests/store_insert_path.rs` |
| Signature verification before persist | `crates/nmp-testing/tests/store_invalid_sig.rs` |
| Duplicate id → merge provenance, keep earliest received_at | `crates/nmp-testing/tests/store_provenance_merge.rs` |
| Replaceable supersession | `crates/nmp-testing/tests/store_replaceable.rs` |
| Parameterized replaceable supersession | `crates/nmp-testing/tests/store_param_replaceable.rs` |
| Kind:5 self-delete persists as tombstone | `crates/nmp-testing/tests/store_kind5_tombstone.rs` |
| Foreign kind:5 ignored | `crates/nmp-testing/tests/store_kind5_foreign.rs` |
| NIP-40 expiration scheduled + reaped | `crates/nmp-testing/tests/store_nip40_expiration.rs` |
| Watermarks survive restart, authoritative cache-miss | `crates/nmp-testing/tests/store_watermarks.rs` |
| Claim register / release; GC drops un-claimed cold | `crates/nmp-testing/tests/store_gc_claims.rs` |
| `nmp dump` round-trip is byte-identical for second dump | `crates/nmp-testing/tests/store_dump_roundtrip.rs` |
| Migration v0→v1 success; rollback on N+1 failure | `crates/nmp-testing/tests/store_domain_migration.rs` |
| Domain isolation: module A cannot read module B's sub-db | `crates/nmp-testing/tests/store_domain_isolation.rs` |
| Working-set ≤ 100 MB at 100 views / 10k hot / 1M cached | `crates/nmp-testing/bin/reactivity-bench` (extended scenario) |

## 12. Performance budget

| Gate | Budget | Measurement |
|---|---|---|
| Cold-start time-to-first-painted-timeline on iPhone 12 (primed LMDB, last session's events on disk) | ≤ 1.5 s p99 | `firehose-bench live cold_start --device iphone12` |
| Cold-start time-to-first-painted-timeline on simulator | ≤ 800 ms p99 (looser than device because no thermal envelope) | same harness, simulator scenario |
| Working-set memory at 100 active views / 10k hot / 1M on disk | ≤ 100 MB resident | Instruments Allocations + `reactivity-bench` working-set scenario |
| Single `insert()` for an unseen kind:1 with 4 secondaries | ≤ 250 µs p99 on iPhone 12 | criterion bench in `crates/nmp-testing/benches/store_insert.rs` |
| `scan_by_author_kind` returning 200 newest events | ≤ 5 ms p99 | criterion bench in `crates/nmp-testing/benches/store_scan.rs` |
| `gc_step()` work-batch ceiling (single call) | ≤ 50 ms total wall time | bounded by `GcBudget { max_events, max_duration_ms }` |
| `nmp dump` of 1M events | sustained ≥ 50k events/sec on M-series Mac | wall-clock measurement in dump-roundtrip test |

Each gate is measurable; any miss revises the design via an ADR before M3 is declared complete (per `plan.md` §1.6 "no silent endings").

## 13. Open questions for ADR after review

1. **`nostr-lmdb` LMDB environment sharing.** Can we open the same `lmdb::Environment` for both `NostrLMDB`'s sub-databases and our own NMP sub-databases (provenance, watermarks, claims, domain rows)? If yes, we get atomic cross-sub-db transactions for free (a single `RwTxn` covers event + provenance + secondary indexes). If `nostr-lmdb` insists on opening its own `Environment`, we lose that and the insert path needs a two-phase write with crash-recovery logic. Investigate before implementation — may require an upstream PR exposing `Environment` access.
2. **Watermark `filter_hash` canonicalisation.** Two `Filter`s that are semantically identical but field-ordered differently must hash the same. The canonicalisation rule (likely: sort all tag-value arrays, sort kinds, sort authors, lexicographic field order before BLAKE3) needs to be specified once and shared with the planner so cache-coverage lookups hit. Candidate: a single `fn canonical_filter_hash(&Filter) -> [u8; 32]` in `nmp-core::store::watermarks`.
3. **Projection cache durability.** Currently in-memory in the existing kernel (`kernel/mod.rs:293` `profiles: HashMap`). Do we persist projection caches as a `DomainModule` or rebuild from events at cold-start? Rebuild is simpler and avoids cache-staleness bugs but adds startup cost; persistence is faster but requires invalidation logic on kind:0 replacement. Recommended default: rebuild on cold-start, measure, decide whether to add the persistence layer in M3.x or M4.
4. **Domain-module per-record encoding.** CBOR via `serde_cbor` vs serde-json vs bincode. CBOR is upstream-compatible (matches `nostr` crate); bincode is faster but stratifies the format. Default: CBOR for cross-language readability; revisit if benchmarks show >5% insert-time cost.
5. **iOS keychain-stored encryption-at-rest key for LMDB.** Out of scope for M3 (mentioned for M6 keychain work) but the schema must not assume cleartext-on-disk forever; reserve a `meta` row for `encryption_version: u32` so a future migration can wrap pages.
6. **`ModuleRegistry::register_domain` API stability.** Adding `DomainFactories` to `ModuleDescriptor` is a non-breaking additive change to the public substrate API (existing callers using only the generic `register_domain::<M>()` continue to compile), but it commits us to keeping `DomainModule::SCHEMA_VERSION` and `DomainModule::migrations` as compile-time-resolvable items rather than object-safe methods. Confirm this with the substrate maintainer before M3 lands — if `DomainModule` is expected to support runtime composition (e.g., plugin loading), we need option (c): the actor passes the live `&[Box<dyn DomainModule>]` to `EventStore::open` instead. Recommended default: stay with `fn`-pointer factories; revisit if a plugin-loading use case appears.

## 14. Citations to current code

- In-memory event store: `crates/nmp-core/src/kernel/mod.rs:294` (`events: HashMap<String, StoredEvent>`), `kernel/mod.rs:46` (`StoredEvent` struct).
- Insert path under wrap: `crates/nmp-core/src/kernel/ingest.rs:166` (`ingest_profile`), `ingest.rs:235` (`ingest_timeline_event`), `ingest.rs:209` (`ingest_relay_list`).
- Replaceable supersession (current scattered logic to be centralised in `EventStore::insert`): `kernel/ingest.rs:166-185` (profile replace by `(pubkey, kind)`), `ingest.rs:218-233` (NIP-65 list replace by `(pubkey, 10002)`).
- Profile claim refcounting (current in-memory analogue of `EventStore::claim/release`): `kernel/mod.rs:315` (`profile_claims: HashMap<String, BTreeSet<String>>`), `kernel/requests.rs:202` (`claim_profile`), `requests.rs:239` (`release_profile`).
- Substrate `DomainModule` trait the store backs: `crates/nmp-core/src/substrate/domain.rs:1` (current shape, lines 1–49).
- Module registry the store consumes at startup: `crates/nmp-core/src/substrate/mod.rs:41` (`ModuleRegistry::register_domain`).

The M3 implementation deletes none of the existing files outright — the kernel's `events: HashMap` and `profiles: HashMap` are replaced by calls to `Box<dyn EventStore>` held inside the `Kernel` struct, and the per-field tests (`kernel/tests.rs`) shift to the new trait via `MemEventStore`. No public FFI surface changes.
