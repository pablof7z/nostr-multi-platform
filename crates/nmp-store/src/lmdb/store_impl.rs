//! `EventStore` trait impl for `LmdbEventStore` (feature = "lmdb-backend").
//!
//! Pure delegation to per-subsystem modules. This file exists so `mod.rs`
//! stays focused on the open() + Inner shape.

use std::collections::{BTreeSet, HashSet};
use std::ops::ControlFlow;

use super::{
    coverage, delete, domain, dump as dump_mod, gc, insert, query, query_relay_index,
    LmdbEventStore,
};
use crate::events::{DomainHandle, EventIter, EventStore};
use crate::types::{
    DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport, InsertOutcome,
    ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, TombstoneRow, VerifiedEvent,
};
use crate::DomainMigration;
use crate::StoreError;

impl EventStore for LmdbEventStore {
    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError> {
        query::get_by_id(&self.inner, id)
    }

    fn scan_by_author_kind<'a>(
        &'a self,
        author: &PubKey,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_author_kind(&self.inner, author, kinds, since, until, limit)
    }

    fn scan_by_authors_kind<'a>(
        &'a self,
        authors: &BTreeSet<PubKey>,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_authors_kind(&self.inner, authors, kinds, since, until, limit)
    }

    fn get_param_replaceable(
        &self,
        pubkey: &PubKey,
        kind: u32,
        d_tag: &[u8],
    ) -> Result<Option<StoredEvent>, StoreError> {
        query::get_param_replaceable(&self.inner, pubkey, kind, d_tag)
    }

    fn scan_by_kind_dtag<'a>(
        &'a self,
        kind: u32,
        d_tag: &[u8],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_kind_dtag(&self.inner, kind, d_tag, since, until, limit)
    }

    fn scan_by_etag<'a>(
        &'a self,
        target: &EventId,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_etag(&self.inner, target, kinds, limit)
    }

    fn scan_by_ptag<'a>(
        &'a self,
        target: &PubKey,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_ptag(&self.inner, target, kinds, limit)
    }

    fn scan_by_kind_time<'a>(
        &'a self,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_kind_time(&self.inner, kinds, since, until, limit)
    }

    fn query_visit(
        &self,
        q: &StoreQuery,
        limit: usize,
        visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
    ) -> Result<(), StoreError> {
        query::query_visit(&self.inner, q, limit, visitor)
    }

    fn scan_expiring_before<'a>(
        &'a self,
        unix_seconds: u64,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_expiring_before(&self.inner, unix_seconds, limit)
    }

    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError> {
        query::tombstones_for(&self.inner, target)
    }

    fn list_tombstones<'a>(
        &'a self,
    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>
    {
        let rows = query::list_tombstones(&self.inner)?;
        Ok(Box::new(rows.into_iter().map(Ok)))
    }

    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError> {
        query::provenance_for(&self.inner, id)
    }

    fn list_events_seen_on(&self, relay_url: &str) -> Result<Vec<EventId>, StoreError> {
        query_relay_index::list_events_seen_on(&self.inner, relay_url)
    }

    fn insert(
        &self,
        event: VerifiedEvent,
        source: &RelayUrl,
        received_at_ms: u64,
    ) -> Result<InsertOutcome, StoreError> {
        insert::insert(&self.inner, event.into_raw(), source, received_at_ms)
    }

    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError> {
        delete::delete_by_filter(&self.inner, filter)
    }

    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
        // No LRU yet — same as Mem.
        Ok(())
    }

    fn gc_step_with_pins(
        &self,
        budget: GcBudget,
        now_secs: u64,
        pins: &HashSet<EventId>,
    ) -> Result<GcReport, StoreError> {
        gc::gc_step(&self.inner, budget, now_secs, pins, &[])
    }

    fn gc_step_with_pins_and_coverage(
        &self,
        budget: GcBudget,
        now_secs: u64,
        pins: &HashSet<EventId>,
        guards: &[crate::types::CoverageGuard],
    ) -> Result<GcReport, StoreError> {
        gc::gc_step(&self.inner, budget, now_secs, pins, guards)
    }

    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle, StoreError> {
        domain::domain_open(&self.inner, namespace)
    }

    fn run_migrations(
        &self,
        namespace: &'static str,
        target_version: u32,
        migrations: &[DomainMigration],
    ) -> Result<(), StoreError> {
        domain::run_migrations(&self.inner, namespace, target_version, migrations)
    }

    fn dump(
        &self,
        out: &mut dyn std::io::Write,
        format: DumpFormat,
    ) -> Result<DumpStats, StoreError> {
        dump_mod::dump(&self.inner, out, format)
    }

    // ─── F-TTL replaceable freshness ───────────────────────────────────────────

    fn get_check_again_after(&self, key: &crate::ReplaceableKey) -> Option<u64> {
        self.inner.lmdb.get_check_again_after(key)
    }

    fn set_check_again_after(&self, key: crate::ReplaceableKey, ts_ms: u64) {
        // D6 graceful degrade: a freshness-stamp failure must never block ingest
        // or claim. A missed stamp just means the next claim re-verifies eagerly.
        if let Err(e) = self.inner.lmdb.set_check_again_after_committed(key, ts_ms) {
            tracing::warn!("F-TTL: set_check_again_after failed (ignored): {e}");
        }
    }

    // ─── K3 coverage ledger (ADR-0056 §3, Stage D1) ────────────────────────────

    fn record_coverage(&self, filter_hash: &str, relay: &str, covered_through: u64) {
        // D6 graceful degrade: a coverage-write failure must never block ingest
        // or the EOSE/NEG-DONE path. A missed write only means the Stage D2 read
        // falls back to the presence floor for this shape — never a wrong floor.
        if let Err(e) = coverage::record_coverage(&self.inner, filter_hash, relay, covered_through) {
            tracing::warn!("K3 coverage: record_coverage failed (ignored): {e}");
        }
    }

    fn get_coverage(&self, filter_hash: &str, relay: &str) -> Option<u64> {
        match coverage::get_coverage(&self.inner, filter_hash, relay) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("K3 coverage: get_coverage failed (treated as None): {e}");
                None
            }
        }
    }

    fn coverage_max_for_filter_hash(&self, filter_hash: &str) -> Option<u64> {
        match coverage::max_for_filter_hash(&self.inner, filter_hash) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("K3 coverage: max_for_filter_hash failed (None): {e}");
                None
            }
        }
    }

    fn coverage_rows_for_filter_hash(&self, filter_hash: &str) -> Vec<(String, u64)> {
        match coverage::rows_for_filter_hash(&self.inner, filter_hash) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("K3 coverage: rows_for_filter_hash failed (empty): {e}");
                Vec::new()
            }
        }
    }
}

// ─── Test-only helpers ────────────────────────────────────────────────────────

#[cfg(all(test, feature = "lmdb-backend"))]
impl LmdbEventStore {
    /// Count the rows in the `nmp-addr-tombstones` sub-db.
    ///
    /// Exposed only for GC tests (S-2 fix) — not part of the public trait.
    pub(super) fn addr_tombstone_count(&self) -> Result<usize, crate::StoreError> {
        let txn = self
            .inner
            .env
            .read_txn()
            .map_err(|e| crate::StoreError::Io(format!("read_txn: {e}")))?;
        let count = self
            .inner
            .addr_tombstones
            .iter(&txn)
            .map_err(|e| crate::StoreError::Io(format!("addr-tomb count iter: {e}")))?
            .count();
        Ok(count)
    }

}
