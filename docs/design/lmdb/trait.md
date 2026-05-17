# LMDB sub-design: `EventStore` trait

> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). This file fixes the trait surface; the master doc fixes the decision.

## 1. Crate placement

`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.

## 2. Supporting types

```rust
use std::sync::Arc;

pub type EventId = [u8; 32];
pub type PubKey = [u8; 32];
pub type RelayUrl = String;

#[derive(Clone, Debug)]
pub struct StoredEvent {
    pub raw: Arc<nostr::Event>,         // upstream nostr crate type
    pub received_at_ms: u64,            // wall-clock first arrival across all relays
}

#[derive(Clone, Debug)]
pub struct ProvenanceEntry {
    pub relay_url: RelayUrl,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub primary: bool,                  // first observed relay (deterministic)
}

#[derive(Clone, Debug)]
pub enum InsertOutcome {
    /// Fresh insert; secondary indexes written.
    Inserted { id: EventId, sources_after: u32 },
    /// Duplicate id; provenance updated, primary untouched.
    Duplicate { id: EventId, sources_after: u32 },
    /// Replaceable supersession: this event replaced an older one.
    Replaced { new_id: EventId, replaced_id: EventId },
    /// Replaceable supersession: incoming was older, dropped.
    Superseded { id: EventId, current_id: EventId },
    /// Suppressed because target is tombstoned.
    Tombstoned { id: EventId, target_kind5_id: EventId },
    /// Signature / delegation / structural validity failed.
    Rejected { id: EventId, reason: RejectReason },
    /// Ephemeral kind: delivered to live consumers, not stored.
    Ephemeral { id: EventId },
}

#[derive(Clone, Debug)]
pub enum RejectReason {
    BadSignature,
    BadDelegation(String),
    Malformed(String),
    ExpiredOnArrival,                   // NIP-40 expiration already in the past
}

#[derive(Clone, Debug)]
pub struct TombstoneRow {
    pub target_id: EventId,
    pub kind5_event_id: Option<EventId>, // None for NIP-40 expiry tombstones
    pub deleter_pubkey: Option<PubKey>,
    pub deleted_at: u64,                 // unix seconds
    pub sources: Vec<RelayUrl>,
    pub origin: TombstoneOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TombstoneOrigin { Kind5, NIP40Expiry, AdminPurge }

#[derive(Clone, Debug)]
pub struct WatermarkKey {
    pub filter_hash: [u8; 32],
    pub relay_url: RelayUrl,
}

#[derive(Clone, Debug)]
pub struct WatermarkRow {
    pub key: WatermarkKey,
    pub synced_up_to: u64,               // unix seconds
    pub last_sync_method: SyncMethod,
    pub last_negentropy_state: Option<Vec<u8>>,
    pub bytes_saved_vs_req: u64,
    pub updated_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncMethod { Negentropy, ReqScan, Manual }

#[derive(Clone, Copy, Debug)]
pub struct ClaimerId(pub u64);           // opaque view-handle id from the actor

#[derive(Clone, Copy, Debug)]
pub struct GcBudget {
    pub max_events_per_step: usize,
    pub max_duration_ms: u32,
}

#[derive(Clone, Debug, Default)]
pub struct GcReport {
    pub expired_reaped: usize,
    pub lru_evicted: usize,
    pub tombstones_purged: usize,
    pub duration_ms: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum DumpFormat { Jsonl, Cbor }

#[derive(Clone, Debug, Default)]
pub struct DumpStats {
    pub events: u64,
    pub tombstones: u64,
    pub watermarks: u64,
    pub domain_rows: u64,
    pub bytes_written: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("backend i/o: {0}")] Io(String),
    #[error("backend corruption: {0}")] Corrupt(String),
    #[error("encoding: {0}")] Encoding(String),
    #[error("schema too new: {namespace} on-disk={on_disk} expected={expected}")]
    SchemaTooNew { namespace: String, on_disk: u32, expected: u32 },
    #[error("schema migration failed: {namespace} v{from}->v{to}: {reason}")]
    MigrationFailed { namespace: String, from: u32, to: u32, reason: String },
    #[error("unknown namespace: {0}")] UnknownNamespace(String),
}
```

The store iterates lazily for scans:

```rust
pub trait EventIter: Iterator<Item = Result<StoredEvent, StoreError>> + Send {}
impl<T: Iterator<Item = Result<StoredEvent, StoreError>> + Send> EventIter for T {}
```

`StoredEvent::raw` is `Arc<nostr::Event>` so the hot LRU can hold reference-counted copies without cloning the event body on each `get_by_id`.

## 3. The trait

```rust
pub trait EventStore: Send + Sync {
    // ─────── Reads ───────

    /// Primary lookup. Returns Ok(None) if absent; tombstones do not count as "present".
    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError>;

    /// `idx_author_kind` scan, newest-first. `kinds` empty = any kind.
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

    /// `idx_etag_time` scan, newest-first. Used by reaction / repost / thread views.
    fn scan_by_etag<'a>(
        &'a self,
        target: &EventId,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_ptag_time` scan, newest-first. Used by notifications / mention views.
    fn scan_by_ptag<'a>(
        &'a self,
        target: &PubKey,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_kind_time` scan, newest-first. Used by timeline backfills.
    /// `kinds` empty = any kind (parity with `scan_by_author_kind`).
    fn scan_by_kind_time<'a>(
        &'a self,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

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
    fn list_watermarks_for_relay<'a>(
        &'a self,
        relay_url: &str,
    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>;

    // ─────── Hot-set / claims (GC) ───────

    /// Register a claim: caller pins `ids` against eviction until `release`.
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

## 4. `DomainHandle`

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

## 5. Error semantics (doctrine D3)

The trait returns `Result<T, StoreError>`. The actor's wrapper functions map them as:

- `Io / Corrupt` at startup → panic (we cannot run without a store; surfaces to platform shell as a process restart).
- `Io / Corrupt` mid-run → `Effect::StoreDegraded { details }` published on the diagnostics bridge (ADR-0007); the affected operation returns the closest-fit graceful default (empty iterator, drop-write); the next gc_step retries.
- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")` per D3.
- `SchemaTooNew` at startup → publish `Effect::DomainSchemaTooNew { namespace }`, the affected module starts in degraded mode (its actions reject with `ActionRejection::ModuleUnavailable`), rest of the kernel runs.
- `MigrationFailed` → same as above, plus a one-time toast on first action attempt.
- `UnknownNamespace` → programming error; assert in debug, log + drop in release.

No `StoreError` ever crosses FFI. The `AppUpdate` carries only successful state + optional `toast: Option<String>`.

## 6. Two backends in v1

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
