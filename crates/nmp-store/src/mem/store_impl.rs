//! `EventStore` trait implementation for `MemEventStore`.
//!
//! Pure delegation — all logic lives in the sub-modules. This file exists so
//! `mod.rs` stays under 200 LOC (Article I hard ceiling).

use std::collections::{BTreeSet, HashSet};
use std::ops::ControlFlow;

use super::{domain, gc, insert, query, MemEventStore};
use crate::events::{DomainHandle, EventIter, EventStore};
use crate::types::{
    DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport, InsertOutcome,
    ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, TombstoneRow, VerifiedEvent,
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

    fn scan_by_authors_kind<'a>(
        &'a self,
        authors: &BTreeSet<PubKey>,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_authors_kind(self, authors, kinds, since, until, limit)
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

    fn relay_kind_coverage(&self, relay_url: &str) -> Result<Vec<u32>, StoreError> {
        let st = self.lock()?;
        Ok(super::relay_kind_coverage(&st, relay_url))
    }

    fn relay_kind_count(&self, relay_url: &str, kind: u32) -> Result<u64, StoreError> {
        let st = self.lock()?;
        Ok(super::relay_kind_count(&st, relay_url, kind))
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

    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
        // Memory backend has no LRU — all events are equally hot. No-op.
        Ok(())
    }

    fn gc_step_with_pins(
        &self,
        budget: GcBudget,
        now_secs: u64,
        pins: &HashSet<EventId>,
    ) -> Result<GcReport, StoreError> {
        gc::gc_step_with_pins(self, budget, now_secs, pins, &[])
    }

    fn gc_step_with_pins_and_coverage(
        &self,
        budget: GcBudget,
        now_secs: u64,
        pins: &HashSet<EventId>,
        guards: &[crate::types::CoverageGuard],
    ) -> Result<GcReport, StoreError> {
        gc::gc_step_with_pins(self, budget, now_secs, pins, guards)
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

    fn interaction_counts(
        &self,
        target: &crate::types::EventId,
    ) -> Result<crate::TargetInteractionCounts, crate::StoreError> {
        let st = self.lock()?;
        let target_hex = super::bytes_to_hex(target);
        let replies = st.interaction_counters
            .get(&(target_hex.clone(), crate::interaction::CounterKind::Reply as u8))
            .copied()
            .unwrap_or(0);
        let reactions = st.interaction_counters
            .get(&(target_hex.clone(), crate::interaction::CounterKind::Reaction as u8))
            .copied()
            .unwrap_or(0);
        let reposts = st.interaction_counters
            .get(&(target_hex.clone(), crate::interaction::CounterKind::Repost as u8))
            .copied()
            .unwrap_or(0);
        let zaps = st.interaction_counters
            .get(&(target_hex, crate::interaction::CounterKind::Zap as u8))
            .copied()
            .unwrap_or(0);
        Ok(crate::TargetInteractionCounts { replies, reactions, reposts, zaps })
    }

    // ─── F-TTL replaceable freshness ───────────────────────────────────────────

    fn get_check_again_after(&self, key: &crate::ReplaceableKey) -> Option<u64> {
        // A poisoned lock degrades to "no record" → the TTL gate treats the
        // identity as due, which is correct-but-eager (never wrong).
        self.lock().ok()?.replaceable_freshness.get(key).copied()
    }

    fn set_check_again_after(&self, key: crate::ReplaceableKey, ts_ms: u64) {
        if let Ok(mut state) = self.lock() {
            state.replaceable_freshness.insert(key, ts_ms);
        }
    }

    // ─── K3 coverage ledger (ADR-0056 §3, Stage D1) ────────────────────────────

    fn record_coverage(&self, filter_hash: &str, relay: &str, covered_through: u64) {
        if let Ok(mut state) = self.lock() {
            let key = (filter_hash.to_string(), relay.to_string());
            let existing = state.coverage.get(&key).copied().unwrap_or(0);
            // Monotonic advance: a later completion can only raise the proven
            // downward-closed bound, never lower it. Only insert when the bound
            // actually advances above 0 — a `covered_through == 0` call (no
            // coverage) must NOT materialise a misleading row, matching the LMDB
            // backend's "write only when `next > existing`" semantics.
            if covered_through > existing {
                state.coverage.insert(key, covered_through);
            }
        }
    }

    fn get_coverage(&self, filter_hash: &str, relay: &str) -> Option<u64> {
        self.lock()
            .ok()?
            .coverage
            .get(&(filter_hash.to_string(), relay.to_string()))
            .copied()
    }

    fn coverage_max_for_filter_hash(&self, filter_hash: &str) -> Option<u64> {
        let state = self.lock().ok()?;
        state
            .coverage
            .iter()
            .filter(|((fh, _relay), _v)| fh == filter_hash)
            .map(|(_k, v)| *v)
            .max()
    }

    fn coverage_rows_for_filter_hash(&self, filter_hash: &str) -> Vec<(String, u64)> {
        let Ok(state) = self.lock() else {
            return Vec::new();
        };
        state
            .coverage
            .iter()
            .filter(|((fh, _relay), _v)| fh == filter_hash)
            .map(|((_fh, relay), v)| (relay.clone(), *v))
            .collect()
    }
}
