//! `EventStore` trait and `DomainHandle` type.
//!
//! Lives in `events.rs` because `trait` is a Rust keyword.
//! See `docs/design/lmdb/trait.md` for the full specification.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};

use super::types::{
    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
    InsertOutcome, ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, TombstoneRow,
    VerifiedEvent, WatermarkKey, WatermarkRow,
};
use super::StoreError;
use crate::substrate::DomainMigration;

// ─── EventIter ────────────────────────────────────────────────────────────────

/// Lazy iterator over stored events — implementations must be `Send` so the
/// planner can page results across thread boundaries.
pub trait EventIter: Iterator<Item = Result<StoredEvent, StoreError>> + Send {}
impl<T: Iterator<Item = Result<StoredEvent, StoreError>> + Send> EventIter for T {}

// ─── DomainHandle ─────────────────────────────────────────────────────────────

/// Shared data map for a single domain namespace (memory backend).
type MemDomainData = Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>;

/// Backend-specific storage for a `DomainHandle`.
pub(crate) enum DomainHandleInner {
    Mem {
        #[allow(dead_code)] // Preserved for debug/domain isolation checks.
        namespace: &'static str,
        data: MemDomainData,
    },
    // LMDB variant — carries the namespace + a handle to the LMDB-side state.
    // The actual storage operations live in `crate::store::lmdb::domain`.
    #[cfg(feature = "lmdb-backend")]
    Lmdb {
        namespace: &'static str,
        backend: Arc<crate::store::lmdb::Inner>,
    },
}

/// Type alias for domain scan iterators.
pub type DomainScanIter<'a> =
    Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>;

/// A module-scoped handle into the domain store for one namespace.
///
/// The kernel does not give a `DraftsModule` handle to `SettingsModule` —
/// isolation is enforced at construction time in `domain_open()`.
///
/// Design: `docs/design/lmdb/trait.md` §3.
pub struct DomainHandle {
    pub(crate) inner: DomainHandleInner,
}

impl DomainHandle {
    /// Write a key/value pair into this domain namespace.
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        match &self.inner {
            DomainHandleInner::Mem { data, .. } => {
                data.lock()
                    .map_err(|e| StoreError::Io(e.to_string()))?
                    .insert(key.to_vec(), value.to_vec());
                Ok(())
            }
            #[cfg(feature = "lmdb-backend")]
            DomainHandleInner::Lmdb { namespace, backend } => {
                crate::store::lmdb::domain::put(backend, namespace, key, value)
            }
        }
    }

    /// Read a value by key from this domain namespace.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        match &self.inner {
            DomainHandleInner::Mem { data, .. } => {
                Ok(data.lock()
                    .map_err(|e| StoreError::Io(e.to_string()))?
                    .get(key)
                    .cloned())
            }
            #[cfg(feature = "lmdb-backend")]
            DomainHandleInner::Lmdb { namespace, backend } => {
                crate::store::lmdb::domain::get(backend, namespace, key)
            }
        }
    }

    /// Delete a key. Returns `true` if the key existed.
    pub fn delete(&self, key: &[u8]) -> Result<bool, StoreError> {
        match &self.inner {
            DomainHandleInner::Mem { data, .. } => {
                Ok(data.lock()
                    .map_err(|e| StoreError::Io(e.to_string()))?
                    .remove(key)
                    .is_some())
            }
            #[cfg(feature = "lmdb-backend")]
            DomainHandleInner::Lmdb { namespace, backend } => {
                crate::store::lmdb::domain::delete(backend, namespace, key)
            }
        }
    }

    /// Scan all entries whose key starts with `prefix`.
    pub fn scan_prefix<'a>(&'a self, prefix: &[u8]) -> Result<DomainScanIter<'a>, StoreError> {
        match &self.inner {
            DomainHandleInner::Mem { data, .. } => {
                let snapshot: Vec<(Vec<u8>, Vec<u8>)> = data
                    .lock()
                    .map_err(|e| StoreError::Io(e.to_string()))?
                    .iter()
                    .filter(|(k, _)| k.starts_with(prefix))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Ok(Box::new(snapshot.into_iter().map(Ok)))
            }
            #[cfg(feature = "lmdb-backend")]
            DomainHandleInner::Lmdb { namespace, backend } => {
                let rows = crate::store::lmdb::domain::scan_prefix(backend, namespace, prefix)?;
                Ok(Box::new(rows.into_iter().map(Ok)))
            }
        }
    }

    /// Scan entries via a named secondary index with the given key prefix.
    pub fn scan_index<'a>(
        &'a self,
        _index: &'static str,
        key_prefix: &[u8],
    ) -> Result<DomainScanIter<'a>, StoreError> {
        // For now both backends have a flat map per namespace — no separate
        // secondary indexes are maintained. Fall back to scan_prefix.
        self.scan_prefix(key_prefix)
    }
}

// ─── EventStore trait ─────────────────────────────────────────────────────────

/// The single storage abstraction for all Nostr events.
///
/// Backends: `MemEventStore` (always), `LmdbEventStore` (feature = "lmdb-backend").
/// See `docs/design/lmdb/trait.md` for invariant documentation.
pub trait EventStore: Send + Sync {
    // ─── Reads ───────────────────────────────────────────────────────────────

    /// Primary lookup. Returns `Ok(None)` if absent; tombstones do not count as "present".
    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError>;

    /// `idx_author_kind` scan, newest-first.
    ///
    /// `kinds` must be non-empty; callers wanting any-kind use `scan_by_kind_time` instead.
    fn scan_by_author_kind<'a>(
        &'a self,
        author: &PubKey,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_kind_dtag` lookup — returns the current parameterized replaceable for
    /// `(pubkey, kind, d_tag)`, or `Ok(None)`.
    fn get_param_replaceable(
        &self,
        pubkey: &PubKey,
        kind: u32,
        d_tag: &[u8],
    ) -> Result<Option<StoredEvent>, StoreError>;

    /// `idx_kind_dtag_time` scan, newest-first across all authors for `(kind, d_tag)`.
    fn scan_by_kind_dtag<'a>(
        &'a self,
        kind: u32,
        d_tag: &[u8],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_etag_time` scan, newest-first. `kinds` must be non-empty.
    fn scan_by_etag<'a>(
        &'a self,
        target: &EventId,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_ptag_time` scan, newest-first. `kinds` must be non-empty.
    fn scan_by_ptag<'a>(
        &'a self,
        target: &PubKey,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// `idx_kind_time` scan, newest-first.
    ///
    /// Pass `kinds = &[]` to scan all kinds (the only scan method that accepts an empty slice).
    fn scan_by_kind_time<'a>(
        &'a self,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// Streaming query: invoke `visitor` once per matching event, newest-first,
    /// up to `limit` events. The visitor returns [`ControlFlow::Break`] to stop
    /// the scan early without materializing the remaining results.
    ///
    /// The visitor receives `&StoredEvent` by reference — no per-event clone or
    /// result-vector allocation occurs on the visit path (D8: working set
    /// bounded, zero per-event alloc after warmup). This default implementation
    /// routes through the matching `scan_by_*` index (so the index logic is not
    /// duplicated); backends may override it to avoid the scan's intermediate
    /// result buffer entirely (see `MemEventStore`).
    ///
    /// Design: `docs/design/nostrdb-notedeck-lessons.md` §2.3 (`ndb_query_visit`).
    fn query_visit(
        &self,
        query: &StoreQuery,
        limit: usize,
        visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
    ) -> Result<(), StoreError> {
        let iter: Box<dyn EventIter + '_> = match query {
            StoreQuery::AuthorKind { author, kinds, since, until } => {
                self.scan_by_author_kind(author, kinds, *since, *until, limit)?
            }
            StoreQuery::KindTime { kinds, since, until } => {
                self.scan_by_kind_time(kinds, *since, *until, limit)?
            }
            StoreQuery::KindDtag { kind, d_tag, since, until } => {
                self.scan_by_kind_dtag(*kind, d_tag, *since, *until, limit)?
            }
            StoreQuery::Etag { target, kinds } => {
                self.scan_by_etag(target, kinds, limit)?
            }
            StoreQuery::Ptag { target, kinds } => {
                self.scan_by_ptag(target, kinds, limit)?
            }
        };
        for item in iter {
            let ev = item?;
            if let ControlFlow::Break(()) = (visitor)(&ev) {
                break;
            }
        }
        Ok(())
    }

    /// Vec-returning query — a thin wrapper over [`query_visit`](Self::query_visit)
    /// so the index logic lives in exactly one place. Materializes matched
    /// events into a `Vec`, newest-first, capped at `limit`.
    fn query(
        &self,
        query: &StoreQuery,
        limit: usize,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        let mut out: Vec<StoredEvent> = Vec::new();
        self.query_visit(query, limit, &mut |ev| {
            out.push(ev.clone());
            ControlFlow::Continue(())
        })?;
        Ok(out)
    }

    /// `idx_expires` scan, ascending — used by the NIP-40 reaper.
    fn scan_expiring_before<'a>(
        &'a self,
        unix_seconds: u64,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;

    /// Tombstones referencing a target id (typically one row).
    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError>;

    /// Iterate all tombstones (used by `nmp dump`).
    fn list_tombstones<'a>(
        &'a self,
    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>;

    /// Provenance sidecar for an event.
    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError>;

    // ─── Writes ──────────────────────────────────────────────────────────────

    /// The single insert path.
    ///
    /// `source` is the relay that delivered this copy. Applies §7.1 invariants,
    /// updates secondaries + provenance + tombstones atomically.
    /// Returns `InsertOutcome` per §7.1.
    ///
    /// Callers must verify the event before calling this method; `VerifiedEvent`
    /// is the proof-of-verification token.
    fn insert(
        &self,
        event: VerifiedEvent,
        source: &RelayUrl,
        received_at_ms: u64,
    ) -> Result<InsertOutcome, StoreError>;

    /// Delete by a NMP-internal filter — for admin / GC / kind:5 application.
    ///
    /// Returns the number of primary rows removed.
    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError>;

    // ─── Watermarks ──────────────────────────────────────────────────────────

    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError>;
    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError>;

    /// Coverage classification for a `(filter, relay)` pair.
    fn coverage(&self, key: &WatermarkKey) -> Result<Coverage, StoreError>;

    /// Iterate watermarks for a specific relay.
    fn list_watermarks_for_relay<'a>(
        &'a self,
        relay_url: &str,
    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>;

    // ─── Hot-set / claims (GC) ───────────────────────────────────────────────

    /// Register the maximum number of events a view may pin at once.
    fn register_view_cover(&self, claimer: ClaimerId, cover_budget: usize) -> Result<(), StoreError>;

    /// Pin `ids` against eviction until `release()`.
    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError>;

    /// Release all pins held by `claimer`.
    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError>;

    /// Soft hint: keep these in hot LRU on a best-effort basis.
    fn hot_set_hint(&self, ids: &[EventId]) -> Result<(), StoreError>;

    /// One bounded GC pass — reap expired, trim LRU, purge old tombstones.
    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError>;

    // ─── Domain rows ─────────────────────────────────────────────────────────

    /// Open a module-scoped domain handle.
    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle, StoreError>;

    /// Run schema migrations for a domain namespace.
    fn run_migrations(
        &self,
        namespace: &'static str,
        target_version: u32,
        migrations: &[DomainMigration],
    ) -> Result<(), StoreError>;

    // ─── Export ──────────────────────────────────────────────────────────────

    /// Dump all store contents in the requested format.
    fn dump(&self, out: &mut dyn std::io::Write, format: DumpFormat) -> Result<DumpStats, StoreError>;
}
