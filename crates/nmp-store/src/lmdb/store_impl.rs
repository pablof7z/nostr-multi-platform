//! `EventStore` trait impl for `LmdbEventStore` (feature = "lmdb-backend").
//!
//! Pure delegation to per-subsystem modules. This file exists so `mod.rs`
//! stays focused on the open() + Inner shape.

use std::ops::ControlFlow;

use super::{claims, delete, domain, dump as dump_mod, gc, insert, query, LmdbEventStore};
use crate::events::{DomainHandle, EventIter, EventStore};
use crate::types::{
    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
    InsertOutcome, ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, TombstoneRow,
    VerifiedEvent, WatermarkKey, WatermarkRow,
};
use crate::StoreError;
use crate::DomainMigration;

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

    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError> {
        query::read_watermark(&self.inner, key)
    }

    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError> {
        query::write_watermark(&self.inner, row)
    }

    fn coverage(&self, key: &WatermarkKey) -> Result<Coverage, StoreError> {
        query::coverage(&self.inner, key)
    }

    fn list_watermarks_for_relay<'a>(
        &'a self,
        relay_url: &str,
    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
    {
        let rows = query::list_watermarks_for_relay(&self.inner, relay_url)?;
        Ok(Box::new(rows.into_iter().map(Ok)))
    }

    fn register_view_cover(
        &self,
        claimer: ClaimerId,
        cover_budget: usize,
    ) -> Result<(), StoreError> {
        claims::register_view_cover(&self.inner, claimer, cover_budget)
    }

    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError> {
        claims::claim(&self.inner, claimer, ids)
    }

    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError> {
        claims::release(&self.inner, claimer)
    }

    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
        // No LRU yet — same as Mem.
        Ok(())
    }

    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
        gc::gc_step(&self.inner, budget)
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
}
