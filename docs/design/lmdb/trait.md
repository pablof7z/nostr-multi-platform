# LMDB sub-design: `EventStore` trait

> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md).
> Supporting types are in [`trait/types.md`](trait/types.md) (extracted to keep this file ≤ 300 LOC).

## 1. Crate placement

`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.

All types referenced below (`InsertOutcome`, `TombstoneRow`, `WatermarkKey`, `ClaimerId`, `StoreError`, etc.) are defined in [`trait/types.md`](trait/types.md) and live in `crates/nmp-core/src/store/types.rs`.

## 2. The trait

```rust
pub trait EventStore: Send + Sync {
    // ─────── Reads ───────

    /// Primary lookup. Returns Ok(None) if absent; tombstones do not count as "present".
    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError>;

    /// `idx_author_kind` scan, newest-first.
    /// `kinds` must be non-empty; callers wanting any-kind use `scan_by_kind_time` instead.
    fn scan_by_author_kind<'a>(
        &'a self,
        author: &PubKey,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_kind_dtag` lookup. Returns the current authoritative parameterized
    /// replaceable for `(pubkey, kind, d_tag)`, or Ok(None).
    fn get_param_replaceable(
        &self,
        pubkey: &PubKey,
        kind: u32,
        d_tag: &[u8],
    ) -> Result<Option<StoredEvent>, StoreError>;

    /// `idx_kind_dtag_time` scan, newest-first across all authors for `(kind, d_tag)`.
    /// Used for global parameterized-replaceable discovery (e.g. "recent articles with slug X").
    /// To list all replaceables by a specific author use `scan_by_author_kind` instead.
    fn scan_by_kind_dtag<'a>(
        &'a self,
        kind: u32,
        d_tag: &[u8],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_etag_time` scan, newest-first. Used by reaction / repost / thread views.
    /// `kinds` must be non-empty; pass `&[7]` for reactions, `&[6]` for reposts, etc.
    fn scan_by_etag<'a>(
        &'a self,
        target: &EventId,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_ptag_time` scan, newest-first. Used by notifications / mention views.
    /// `kinds` must be non-empty.
    fn scan_by_ptag<'a>(
        &'a self,
        target: &PubKey,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_kind_time` scan, newest-first. Pass `kinds = &[]` to scan all kinds
    /// (the only scan method that accepts an empty kinds slice).
    fn scan_by_kind_time<'a>(
        &'a self,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// Streaming query: invoke `visitor` per matching event, newest-first, up
    /// to `limit`. The visitor returns `ControlFlow::Break` to stop the scan
    /// early. The visitor receives `&StoredEvent` — no per-event clone or
    /// result-vec allocation on the visit path (D8). Default impl routes
    /// through the matching `scan_by_*` index; backends may override to skip
    /// the scan's intermediate buffer (MemEventStore does).
    /// Design: `docs/design/nostrdb-notedeck-lessons.md` §2.3.
    fn query_visit(
        &self,
        query: &StoreQuery,
        limit: usize,
        visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
    ) -> Result<(), StoreError> { /* default: scan_by_* + visit */ }

    /// Vec-returning query — a thin wrapper over `query_visit` so the index
    /// logic lives in exactly one place.
    fn query(&self, query: &StoreQuery, limit: usize)
        -> Result<Vec<StoredEvent>, StoreError> { /* default: collect via query_visit */ }

    /// `idx_expires` scan, ascending — used by the NIP-40 reaper.
    fn scan_expiring_before<'a>(
        &'a self,
        unix_seconds: u64,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// Tombstones referencing a target id (typically one row).
    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError>;

    /// Iterate all tombstones (used by `nmp dump`).
    fn list_tombstones<'a>(&'a self)
        -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>;

    /// Provenance sidecar for an event.
    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError>;

    // ─────── Writes ───────

    /// The single insert path. `source` is the relay that delivered this copy.
    /// Verifies signature/delegation, applies §7.1 invariants, updates secondaries
    /// + provenance + tombstones atomically. Returns InsertOutcome per §7.1.
    fn insert(&self, event: nostr::Event, source: &RelayUrl, received_at_ms: u64)
        -> Result<InsertOutcome, StoreError>;

    /// Delete by a NMP-internal filter — for admin / GC / kind:5 application.
    /// Returns the number of primary rows removed.
    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError>;

    // ─────── Watermarks ───────

    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError>;
    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError>;

    /// Returns the coverage classification for a `(filter, relay)` pair
    /// based on the stored watermark row and the configured staleness window.
    /// Used by the M2 subscription planner to decide whether a cache miss is
    /// authoritative (no need to fetch) or requires a new REQ.
    fn coverage(&self, key: &WatermarkKey) -> Result<Coverage, StoreError>;

    /// Iterate watermarks for a specific relay. O(matching rows) — backed by the
    /// `idx_watermark_relay` secondary index (see [`keys.md`](keys.md) §5).
    fn list_watermarks_for_relay<'a>(
        &'a self,
        relay_url: &str,
    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>;

    // ─────── Hot-set / claims (GC) ───────

    /// Register the maximum number of events this view is allowed to pin at once.
    /// Must be called before `claim()` for a given `claimer`. If not called,
    /// the store applies a default per-view ceiling of `max_claim_per_view` events
    /// (see [`gc.md`](gc.md) §2 — default 1 000).
    ///
    /// Enforcement: `claim()` counts the current per-claimer set size; if adding
    /// `ids` would exceed this budget OR the global `max_pinned_total` ceiling,
    /// it returns `Err(StoreError::OverPinned { ... })` without modifying state.
    /// The caller is responsible for releasing stale claims first.
    ///
    /// Rationale: D8 (reactivity contract) requires that the kernel's working-set
    /// is bounded at all times. An unbounded pin overlay would let a misbehaving
    /// view module inflate memory without limit (ADR-0001..0004).
    fn register_view_cover(&self, claimer: ClaimerId, cover_budget: usize) -> Result<(), StoreError>;

    /// Pin `ids` against eviction until `release()`. Returns `StoreError::OverPinned`
    /// if adding `ids` would exceed the per-claimer budget or the global ceiling.
    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError>;
    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError>;

    /// Soft hint: keep these in hot LRU on a best-effort basis.
    fn hot_set_hint(&self, ids: &[EventId]) -> Result<(), StoreError>;

    /// One bounded GC pass — reap expired, trim LRU, purge old tombstones.
    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError>;

    // ─────── Domain rows (per-DomainModule typed namespace) ───────

    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle<'_>, StoreError>;
    fn run_migrations(&self, namespace: &'static str, target_version: u32,
                      migrations: &[crate::substrate::DomainMigration])
        -> Result<(), StoreError>;

    // ─────── Export ───────

    fn dump(&self, out: &mut dyn std::io::Write, format: DumpFormat)
        -> Result<DumpStats, StoreError>;
}
```

`DeleteFilter` mirrors the limited subset of admin operations the kernel needs (by-relay-only events, by-author, by-id-list, by-kind range); it is **not** a pass-through to `nostr::Filter` — we intentionally do not expose arbitrary remote filters as a delete vector.

`Coverage` (returned by `coverage()`):

```rust
pub enum Coverage {
    CompleteAsOf(u64),  // fully synced; a cache miss is authoritative "doesn't exist"
    PartialUpTo(u64),   // synced up to timestamp but row is stale — fetch is needed
    Unknown,            // no watermark; always fetch
}
```

## 3. `DomainHandle`

```rust
pub struct DomainHandle<'env> {
    pub(crate) namespace: &'static str,
    pub(crate) inner: DomainHandleInner<'env>,  // backend-specific
}

impl<'env> DomainHandle<'env> {
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError>;
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;
    pub fn delete(&self, key: &[u8]) -> Result<bool, StoreError>;
    pub fn scan_prefix<'a>(&'a self, prefix: &[u8])
        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
    pub fn scan_index<'a>(&'a self, index: &'static str, key_prefix: &[u8])
        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
}
```

A handle is module-scoped; the kernel does not give a `DraftsModule` handle to `SettingsModule` (per `kernel-substrate.md` §8 "Domain stores are isolated"). The handle is `'env`-bounded so it cannot outlive the LMDB environment.

## 4. Error semantics (doctrine D6)

The trait returns `Result<T, StoreError>`. The actor's wrapper functions map them as:

- `Io / Corrupt` at startup → panic (we cannot run without a store; surfaces to platform shell as a process restart).
- `Io / Corrupt` mid-run → `Effect::StoreDegraded { details }` published on the diagnostics bridge (ADR-0007); the affected operation returns the closest-fit graceful default (empty iterator, drop-write); the next gc_step retries.
- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")`.
- `SchemaTooNew` at startup → publish `Effect::DomainSchemaTooNew { namespace }`, the affected module starts in degraded mode (its actions reject with `ActionRejection::ModuleUnavailable`), rest of the kernel runs.
- `MigrationFailed` → same as above, plus a one-time toast on first action attempt.
- `UnknownNamespace` → programming error; assert in debug, log + drop in release.
- `OverPinned` → the caller (actor) surfaces this as `Effect::ViewOverPinned { claimer }` and then calls `release(claimer)` to drop the offending claim, keeping the working set bounded per D8.

No `StoreError` ever crosses FFI (D6). The `AppUpdate` carries only successful state + optional `toast: Option<String>`.

## 5. Two backends in v1

```rust
// In-memory backend, kept for tests + web-pre-M15.
pub struct MemEventStore { /* HashMap-backed; preserves the current kernel state */ }

// Production backend on iOS / Android / Desktop.
pub struct LmdbEventStore { /* wraps nostr_lmdb::NostrLMDB + NMP sub-dbs */ }

pub fn open_event_store(cfg: &AppConfig) -> Result<Box<dyn EventStore>, StoreError> {
    match cfg.storage_backend {
        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
        StorageBackend::Lmdb { ref path } => Ok(Box::new(LmdbEventStore::open(path)?)),
    }
}
```

`MemEventStore` implements every method using `HashMap` / `BTreeMap`. The same test suite runs against both backends with `#[cfg(feature = "lmdb-backend")]` gating only the LMDB-specific edge tests (corruption recovery, oversized values).
