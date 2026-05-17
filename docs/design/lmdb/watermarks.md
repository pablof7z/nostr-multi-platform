# LMDB sub-design: watermarks, provenance, migrations

> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Covers the three NMP-owned LMDB sub-databases that hold non-event durable state: `watermarks`, `provenance`, and the per-`DomainModule` sub-databases tracked by `_meta`.

## 1. Watermark table

Sub-db: `watermarks`. Populated by the M4 NIP-77 negentropy engine and consulted by the M2 subscription planner (per `docs/product-spec/subsystems.md` §7.2 "coverage-aware backfill").

Key: `filter_hash[32] ‖ relay_url_bytes` — exact-key lookups only; no scans by `filter_hash` alone are needed (the planner always knows the relay it's about to query). The relay url is appended raw (no length prefix) because lookups are exact-key and the trailing bytes have no semantic role beyond identification.

Value: CBOR `WatermarkRow`:

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WatermarkRow {
    pub key: WatermarkKey,
    pub synced_up_to: u64,                 // unix seconds
    pub last_sync_method: SyncMethod,      // Negentropy | ReqScan | Manual
    pub last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob (M4)
    pub bytes_saved_vs_req: u64,           // cumulative; for diagnostics
    pub updated_at: u64,                   // unix seconds
}
```

`last_negentropy_state` is an opaque byte blob written by the NIP-77 engine (M4) — the store does not interpret it. Empty for `ReqScan` / `Manual` rows.

### 1.1 Authoritative cache-miss semantics

Per §7.1 of the spec: "A cache-miss query against a fully-synced `(filter, relay)` pair is **authoritative**." The store implements this via the read path:

```rust
pub fn coverage(&self, key: &WatermarkKey, now_s: u64) -> Coverage {
    match self.read_watermark(key) {
        Ok(Some(row)) if row.synced_up_to >= now_s.saturating_sub(self.cfg.coverage_staleness_secs) =>
            Coverage::CompleteAsOf(row.synced_up_to),
        Ok(Some(row)) => Coverage::PartialUpTo(row.synced_up_to),
        Ok(None) => Coverage::Unknown,
        Err(_) => Coverage::Unknown,  // degraded; do not lie about completeness
    }
}
```

`coverage_staleness_secs` defaults to 300 s — a row that hasn't been re-confirmed in 5 minutes is treated as partial. The planner uses this signal to decide whether a missing-event query is "doesn't exist" (CompleteAsOf) or "need to fetch" (PartialUpTo / Unknown).

### 1.2 Restart hydration

On `LmdbEventStore::open()`, the store reads all `watermarks` rows and builds an in-memory `HashMap<WatermarkKey, WatermarkRow>` for hot lookups. Every `write_watermark` updates both the in-memory map and the LMDB row in a single `RwTxn`. Restart re-derives the map; we don't need a separate cache file.

For installations with O(100k+) watermarks (an edge case — typical apps see O(100)–O(10k)), the in-memory map switches to a lazy-loaded variant that pages on demand. Threshold and switching logic deferred to M4 when the negentropy engine's real-world row count is measured.

## 2. Provenance

Sub-db: `provenance`. Per-event sidecar; the master doc §9 justifies the split-table choice.

Key: `event_id[32]`. Value: CBOR `ProvenanceRow`:

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProvenanceRow {
    pub sources: SmallVec<[ProvenanceEntry; 4]>,    // bounded at 32; see master doc §9
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProvenanceEntry {
    pub relay_url: String,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub primary: bool,
}
```

The `primary: bool` is set deterministically: after every mutation, the `sources` vec is sorted by `(first_seen_ms, relay_url)` and the head element gets `primary = true`, all others `false`. This produces a stable "first observed relay" identifier regardless of the order in which redeliveries are processed.

### 2.1 Mutation hot path

For a duplicate-id insert, the per-event provenance write is the **only** LMDB write (per [`keys.md`](keys.md) §9 worked example). The store reads the existing row, mutates the matching entry's `last_seen_ms` (or appends), re-sorts + recomputes `primary`, and writes it back. Total cost: 1 read + 1 write on a 4-element CBOR row — well under 50 µs on iPhone 12 NAND.

The bound of 32 distinct relays per event is empirical: in practice an event is delivered by 1–6 relays; events propagated to "everywhere" (popular kind:0 / kind:3) might hit 15–25. The 32nd entry overwrites the oldest non-primary slot, preserving the primary anchor.

### 2.2 Reuse in outbox routing (M2)

The M2 subscription planner consults `provenance_for(id)` to learn which relays have already delivered an event when scoring per-relay coverage in `LogicalInterestStatus::relay_urls`. This avoids re-fetching the same event from relays we already know carry it. The provenance sidecar is also part of the ADR-0007 diagnostics bridge — the diagnostics screen shows per-event source counts in the firehose tap view.

## 3. Filter canonicalisation (for `filter_hash`)

The `filter_hash` field in `WatermarkKey` is BLAKE3 of the canonical filter encoding. Canonicalisation rules:

1. Within each tag-value array (`#e`, `#p`, `#a`, etc.), sort ascending bytewise.
2. Sort the `kinds` array ascending numerically.
3. Sort the `authors` array ascending bytewise.
4. Sort the `ids` array ascending bytewise.
5. Encode the filter as CBOR with map keys in this lexicographic order: `authors`, `ids`, `kinds`, `since`, `until`, `limit`, `search`, then `#<tag>` keys in ascending tag-letter order.
6. BLAKE3-hash the resulting bytes.

This produces a deterministic hash that is stable across `Filter` field-order variations and across Rust HashMap ordering randomness. The implementation lives at `crates/nmp-core/src/store/watermarks.rs::canonical_filter_hash(&Filter) -> [u8; 32]` and is the single source of truth for the planner + sync engine + dump format.

A filter with `limit: Some(N)` produces a *different* hash than the same filter without `limit` — because their cache-coverage semantics genuinely differ. A planner that wants to share a watermark across "limit=200" and "limit=500" requests of the same shape strips `limit` before hashing (this is a planner-side optimisation, not a store-side one).

## 4. Migration plumbing

Per `kernel-substrate.md` §2: `DomainModule` declares `SCHEMA_VERSION` and `migrations()`. The store handles applying them at startup.

### 4.1 Registry extension required

The existing `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:36-79`) stores only `ModuleDescriptor { namespace, family, rust_type }` — the concrete `M: DomainModule` type is consumed by the generic `register_domain::<M>()` call and not retained, so the store has no runtime path from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()`. M3 extends `ModuleDescriptor` for the Domain family with two `fn`-pointer factories — matching the existing `DomainIndex::key_fn: fn(&[u8]) -> ...` pattern (`substrate/domain.rs:18`):

```rust
// Added in M3 — substrate/domain.rs
pub struct DomainFactories {
    pub schema_version: fn() -> u32,
    pub migrations: fn() -> Vec<DomainMigration>,
    pub indexes: fn() -> Vec<DomainIndex>,
}

// ModuleRegistry::register_domain becomes:
pub fn register_domain<M: DomainModule>(&mut self) {
    let factories = DomainFactories {
        schema_version: || M::SCHEMA_VERSION,
        migrations: M::migrations,
        indexes: M::indexes,
    };
    self.push_domain::<M>(M::NAMESPACE, factories);
}
```

The store reads these factories at open time. No `Box<dyn DomainModule>` is required, no trait object-safety constraints are imposed on `DomainModule`, and the change is additive to the existing trait.

### 4.2 Startup sequence

```rust
pub fn open(path: &Path, modules: &ModuleRegistry) -> Result<Self, StoreError> {
    let env = open_lmdb_environment(path)?;
    let meta = env.open_db(Some("_meta"))?;
    let mut store = Self::bootstrap(env)?;

    // _kernel schema version
    store.migrate_kernel_schema(&meta)?;

    // each registered DomainModule
    for (namespace, factories) in modules.domain_factories() {
        let current = store.read_meta_schema_version(namespace)?;
        let target = (factories.schema_version)();
        let mut applied = current;
        let mut steps = (factories.migrations)();
        steps.retain(|m| m.from_version >= current && m.to_version <= target);
        steps.sort_by_key(|m| m.from_version);
        for step in steps {
            store.run_migration_step(namespace, step)?;
            applied = step.to_version;
            store.write_meta_schema_version(namespace, applied)?;
        }
        if applied < target {
            return Err(StoreError::MigrationFailed { /* missing step */ });
        }
        if applied > target {
            return Err(StoreError::SchemaTooNew { /* downgrade */ });
        }
    }
    Ok(store)
}
```

Each `run_migration_step` opens its own `RwTxn`, calls `step.apply(&mut migration_tx)`, drains `migration_tx.writes()` into the relevant sub-db, and commits. Either the whole step lands atomically or LMDB rolls it back on commit failure.

### 4.3 Rollback semantics

LMDB does not support cross-process downgrade; once `_meta.<namespace>.schema_version` is bumped, there is no "undo." Therefore:

- If migration step N fails: `_meta` is **not** bumped; module starts in degraded mode (per [`trait.md`](trait.md) §5); user-visible diagnostic surfaces the failure.
- If migration step N succeeds but N+1 fails: `_meta` is at N (the highest successful step). The module is "partly migrated"; the same degraded-mode handling applies; on next startup the runner retries from N → N+1.
- If the user actually needs to downgrade (a forensics use case), they delete the sub-db and re-sync from relays. The `nmp dump` format is the supported escape hatch.

### 4.4 Cross-module migration coordination (deferred — see open questions)

`kernel-substrate.md` §12 already flags "cross-module migration coordination" as an open question. M3 implements the per-module path; cross-module coordination is deferred to a future module that needs it. The trait surface does not preclude adding it later: a future `Vec<NamespaceDependency>` field on `DomainMigration` would let the runner topologically order modules' migrations.

## 5. The kernel store's own schema

The kernel's internal LMDB layout (provenance + secondaries + tombstones + watermarks) is itself versioned via the reserved `_kernel` namespace in `_meta`. Current version: `1`. A future schema change (e.g., a new secondary index) bumps this and runs a one-time rebuild from primary events.

The schema migration for `_kernel` v1 → v2 is privileged: it can read every sub-db and rewrite as needed. It runs **before** any `DomainModule` migration so module code never sees a stale kernel schema.

## 6. Diagnostics surface

`WatermarkRow` and `ProvenanceRow` are read by the ADR-0007 diagnostics bridge:

- `LogicalInterestStatus.cache_coverage` resolves from `coverage()` (§1.1 above) for the relevant `(filter_hash, relay_url)` pair.
- The firehose-tap diagnostic surfaces per-event provenance: which relays contributed each row, with `primary` flagged.
- `StoreHealth.watermark_count` (per [`gc.md`](gc.md) §7) summarises the global count.

These are read-mostly paths; the only contention is between the actor's `write_watermark` calls (driven by M4 sync completions) and the diagnostics query, which is bounded by LMDB's MVCC reader semantics — no lock contention.
