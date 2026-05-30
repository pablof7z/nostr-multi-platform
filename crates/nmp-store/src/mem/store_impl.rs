//! `EventStore` trait implementation for `MemEventStore`.
//!
//! Pure delegation — all logic lives in the sub-modules. This file exists so
//! `mod.rs` stays under 200 LOC (Article I hard ceiling).

use std::ops::ControlFlow;

use super::{domain, gc, insert, query, MemEventStore};
use crate::events::{DomainHandle, EventIter, EventStore};
use crate::types::{
    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
    InsertOutcome, ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, TombstoneRow,
    VerifiedEvent, WatermarkKey, WatermarkRow,
};
use crate::DomainMigration;
use crate::StoreError;

impl EventStore for MemEventStore {
    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError> {
        query::get_by_id(self, id)
    }

    fn scan_by_author_kind<'a>(
        &'a self,
        author: &PubKey,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_author_kind(self, author, kinds, since, until, limit)
    }

    fn get_param_replaceable(
        &self,
        pubkey: &PubKey,
        kind: u32,
        d_tag: &[u8],
    ) -> Result<Option<StoredEvent>, StoreError> {
        query::get_param_replaceable(self, pubkey, kind, d_tag)
    }

    fn scan_by_kind_dtag<'a>(
        &'a self,
        kind: u32,
        d_tag: &[u8],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_kind_dtag(self, kind, d_tag, since, until, limit)
    }

    fn scan_by_etag<'a>(
        &'a self,
        target: &EventId,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_etag(self, target, kinds, limit)
    }

    fn scan_by_ptag<'a>(
        &'a self,
        target: &PubKey,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_ptag(self, target, kinds, limit)
    }

    fn scan_by_kind_time<'a>(
        &'a self,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_kind_time(self, kinds, since, until, limit)
    }

    fn query_visit(
        &self,
        q: &StoreQuery,
        limit: usize,
        visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
    ) -> Result<(), StoreError> {
        query::query_visit(self, q, limit, visitor)
    }

    fn scan_expiring_before<'a>(
        &'a self,
        unix_seconds: u64,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_expiring_before(self, unix_seconds, limit)
    }

    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError> {
        query::tombstones_for(self, target)
    }

    fn list_tombstones<'a>(
        &'a self,
    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>
    {
        query::list_tombstones(self)
    }

    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError> {
        query::provenance_for(self, id)
    }

    fn list_events_seen_on(&self, relay_url: &str) -> Result<Vec<EventId>, StoreError> {
        let st = self.lock()?;
        Ok(super::list_seen_on(&st, relay_url))
    }

    fn insert(
        &self,
        event: VerifiedEvent,
        source: &RelayUrl,
        received_at_ms: u64,
    ) -> Result<InsertOutcome, StoreError> {
        insert::insert(self, event.into_raw(), source, received_at_ms)
    }

    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError> {
        insert::delete_by_filter(self, filter)
    }

    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError> {
        query::read_watermark(self, key)
    }

    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError> {
        query::write_watermark(self, row)
    }

    fn coverage(&self, key: &WatermarkKey, now_secs: u64) -> Result<Coverage, StoreError> {
        query::coverage(self, key, now_secs)
    }

    fn list_watermarks_for_relay<'a>(
        &'a self,
        relay_url: &str,
    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
    {
        query::list_watermarks_for_relay(self, relay_url)
    }

    fn register_view_cover(
        &self,
        claimer: ClaimerId,
        cover_budget: usize,
    ) -> Result<(), StoreError> {
        gc::register_view_cover(self, claimer, cover_budget)
    }

    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError> {
        gc::claim(self, claimer, ids)
    }

    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError> {
        gc::release(self, claimer)
    }

    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
        // Memory backend has no LRU — all events are equally hot. No-op.
        Ok(())
    }

    fn gc_step(&self, budget: GcBudget, now_secs: u64) -> Result<GcReport, StoreError> {
        gc::gc_step(self, budget, now_secs)
    }

    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle, StoreError> {
        domain::domain_open(self, namespace)
    }

    fn run_migrations(
        &self,
        namespace: &'static str,
        target_version: u32,
        migrations: &[DomainMigration],
    ) -> Result<(), StoreError> {
        domain::run_migrations(self, namespace, target_version, migrations)
    }

    fn dump(
        &self,
        out: &mut dyn std::io::Write,
        format: DumpFormat,
    ) -> Result<DumpStats, StoreError> {
        query::dump(self, out, format)
    }
}
