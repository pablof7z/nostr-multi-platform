//! `EventStore` trait impl for `LmdbEventStore` (feature = "lmdb-backend").
//!
//! Pure delegation to per-subsystem modules. This file exists so `mod.rs`
//! stays focused on the open() + Inner shape.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::ControlFlow;

use super::{
    coverage, delete, domain, dump as dump_mod, fts, gc, ingest_log, insert, query,
    query_relay_index, LmdbEventStore,
};
use crate::domain_handle::DomainHandle;
use crate::events::{EventIter, EventStore};
use crate::ingest_log::ScanLogResult;
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

    fn peek_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError> {
        query::peek_by_id(&self.inner, id)
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

    fn scan_by_tags<'a>(
        &'a self,
        authors: &BTreeSet<PubKey>,
        kinds: &[u32],
        tags: &BTreeMap<nostr::SingleLetterTag, BTreeSet<String>>,
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        query::scan_by_tags(&self.inner, authors, kinds, tags, since, until, limit)
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

    fn relay_kind_coverage(&self, relay_url: &str) -> Result<Vec<u32>, StoreError> {
        query_relay_index::relay_kind_coverage(&self.inner, relay_url)
    }

    fn relay_kind_count(&self, relay_url: &str, kind: u32) -> Result<u64, StoreError> {
        query_relay_index::relay_kind_count(&self.inner, relay_url, kind)
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

    // ─── Full-text search (issue #1811) — durable LMDB inverted index ───────────

    fn install_search_index_specs(&self, specs: Vec<crate::text_search::CompiledIndexSpec>) {
        // Composition-time install (single writer). Store the spec set on Inner,
        // then run the one-time / tokenizer-version backfill so the durable index
        // is consistent with the spec set before the first query. A poisoned lock
        // degrades to "no FTS" (search then returns Unsupported) — never a panic.
        match self.inner.fts_specs.write() {
            Ok(mut g) => *g = specs,
            Err(e) => {
                tracing::warn!(error = %e, "install_search_index_specs: fts_specs lock poisoned");
                return;
            }
        }
        if let Err(e) = fts::backfill_fts_index(&self.inner) {
            tracing::warn!("FTS backfill failed (search degraded to IndexBuilding/empty): {e}");
        }
    }

    fn cache_search_scopes(&self) -> Vec<(crate::text_search::SearchScopeId, BTreeSet<u32>)> {
        // Mirrors the mem backend: the cache-serve hook reads the installed
        // cache-eligible scopes so a search shape whose kinds intersect a scope
        // is served from this durable inverted index instead of relay-only.
        self.inner.fts_cache_scopes()
    }

    fn text_search_visit(
        &self,
        query: &crate::text_search::TextSearchQuery,
        visitor: &mut dyn FnMut(crate::text_search::TextSearchHit) -> ControlFlow<()>,
    ) -> Result<crate::text_search::TextSearchStatus, StoreError> {
        fts::text_search_visit(&self.inner, query, visitor)
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

    // ─── K3 coverage ledger (ADR-0072 §3, Stage D1) ────────────────────────────

    fn record_coverage(&self, filter_hash: &str, relay: &str, covered_through: u64) {
        // D6 graceful degrade: a coverage-write failure must never block ingest
        // or the EOSE/NEG-DONE path. A missed write only means the Stage D2 read
        // falls back to the presence floor for this shape — never a wrong floor.
        if let Err(e) = coverage::record_coverage(&self.inner, filter_hash, relay, covered_through)
        {
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
    // ─── Ingest log (ADR-0072 §3, step 1) ────────────────────────────────────────

    fn latest_ingest_seq(&self) -> Result<u64, StoreError> {
        ingest_log::latest_seq(self.inner.ingest_meta, &self.inner.env)
    }

    fn oldest_available_seq(&self) -> Result<Option<u64>, StoreError> {
        ingest_log::oldest_seq(
            self.inner.ingest_log,
            self.inner.ingest_meta,
            &self.inner.env,
        )
    }

    fn scan_log_since_seq(
        &self,
        after_seq: u64,
        limit: usize,
    ) -> Result<ScanLogResult, StoreError> {
        ingest_log::scan_since(
            self.inner.ingest_log,
            self.inner.ingest_meta,
            &self.inner.env,
            after_seq,
            limit,
        )
    }

    fn replace_log_retention_claims(&self, claims: &[crate::ingest_log::LogRetentionClaim]) {
        // ADR-0072 §6 step-4: single-writer (kernel) wholesale replace of the
        // VOLATILE claim set. A poisoned lock is non-fatal — a missed update only
        // means the next append-time trim uses the prior set; the consumer
        // degrades to an explicit PullGap, never a silent skip.
        match self.inner.retention_claims.write() {
            Ok(mut g) => *g = claims.to_vec(),
            Err(e) => tracing::warn!(
                error = %e,
                "replace_log_retention_claims: retention_claims lock poisoned (claims not updated)"
            ),
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
