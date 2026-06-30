//! `OpfsSqliteEventStore` — the `EventStore` impl wrapping the wasm32-only
//! OPFS-SQLite engine (`nmp_sqlite_wasm::OpfsSqliteStore`) (#1007).
//!
//! This is the engine→kernel keystone: the engine carries crate-local mirror
//! types and cannot depend on `nmp-store` (Cargo cycle), so the `EventStore`
//! trait impl lives here (orphan rule — the trait owner is `nmp-store`),
//! wrapping the engine handle and adapting every type at the [`super::conv`]
//! seam. With this, the engine becomes usable as `Arc<dyn EventStore>` for
//! kernel injection (PR-7's `nmp-browser-runtime`).
//!
//! Mirrors `nmp-store/src/lmdb/store_impl.rs`: pure delegation, one engine
//! call + one conversion per method. Scans materialize the engine's
//! `Vec<StoredEngineEvent>` and hand back a `Box<dyn EventIter>` over
//! `Result<StoredEvent, _>` (the `Send` bound holds — `StoredEngineEvent` is
//! `Send`). `query_visit` delegates to the engine's budgeted index walk,
//! adapting the visitor.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::ControlFlow;
use std::sync::Arc;

use nmp_sqlite_wasm::OpfsSqliteStore;
use nostr::SingleLetterTag;

use super::conv;
use crate::domain_handle::{DomainHandle, DomainHandleInner};
use crate::events::{EventIter, EventStore};
use crate::ingest_log::{LogRetentionClaim, ScanLogResult};
use crate::types::{
    CoverageGuard, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport, InsertOutcome,
    ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, TombstoneRow, VerifiedEvent,
};
use crate::DomainMigration;
use crate::MigrationTx;
use crate::ReplaceableKey;
use crate::StoreError;

/// The `EventStore` backend over the OPFS-SQLite engine (browser-durable).
///
/// Holds the engine handle in an `Arc` so module-scoped [`DomainHandle`]s can
/// share it (mirroring the LMDB backend's `Arc<Inner>`). Construct with
/// [`OpfsSqliteEventStore::open`].
pub struct OpfsSqliteEventStore {
    inner: Arc<OpfsSqliteStore>,
}

impl OpfsSqliteEventStore {
    /// Open (and schema-create) the OPFS-SQLite store named `database_name`.
    ///
    /// Delegates to the engine's one-time async pool-open (ADR-0054 §1);
    /// PR-7's `nmp-browser-runtime` calls this before `Start`. Every later
    /// `EventStore` method is synchronous over the returned handle.
    pub async fn open(database_name: &str) -> Result<Self, StoreError> {
        let inner = OpfsSqliteStore::open(database_name)
            .await
            .map_err(conv::store_err)?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Materialize an engine scan `Vec` into a boxed `EventIter` of
    /// `Result<StoredEvent, _>` (`Send` — `StoredEngineEvent` is `Send`).
    fn boxed_scan<'a>(rows: Vec<nmp_sqlite_wasm::StoredEngineEvent>) -> Box<dyn EventIter + 'a> {
        Box::new(rows.into_iter().map(|se| Ok(conv::stored_into(se))))
    }
}

impl EventStore for OpfsSqliteEventStore {
    // ─── Reads ────────────────────────────────────────────────────────────────

    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError> {
        Ok(self
            .inner
            .get_by_id(id)
            .map_err(conv::store_err)?
            .map(conv::stored_into))
    }

    fn peek_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError> {
        Ok(self
            .inner
            .peek_by_id(id)
            .map_err(conv::store_err)?
            .map(conv::stored_into))
    }

    fn scan_by_author_kind<'a>(
        &'a self,
        author: &PubKey,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let rows = self
            .inner
            .scan_by_author_kind(author, kinds, since, until, limit)
            .map_err(conv::store_err)?;
        Ok(Self::boxed_scan(rows))
    }

    fn scan_by_authors_kind<'a>(
        &'a self,
        authors: &BTreeSet<PubKey>,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let rows = self
            .inner
            .scan_by_authors_kind(authors, kinds, since, until, limit)
            .map_err(conv::store_err)?;
        Ok(Self::boxed_scan(rows))
    }

    fn get_param_replaceable(
        &self,
        pubkey: &PubKey,
        kind: u32,
        d_tag: &[u8],
    ) -> Result<Option<StoredEvent>, StoreError> {
        Ok(self
            .inner
            .get_param_replaceable(pubkey, kind, d_tag)
            .map_err(conv::store_err)?
            .map(conv::stored_into))
    }

    fn scan_by_kind_dtag<'a>(
        &'a self,
        kind: u32,
        d_tag: &[u8],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let rows = self
            .inner
            .scan_by_kind_dtag(kind, d_tag, since, until, limit)
            .map_err(conv::store_err)?;
        Ok(Self::boxed_scan(rows))
    }

    fn scan_by_tags<'a>(
        &'a self,
        authors: &BTreeSet<PubKey>,
        kinds: &[u32],
        tags: &BTreeMap<SingleLetterTag, BTreeSet<String>>,
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let engine_tags = conv::tags_into_engine(tags);
        let rows = self
            .inner
            .scan_by_tags(authors, kinds, &engine_tags, since, until, limit)
            .map_err(conv::store_err)?;
        Ok(Self::boxed_scan(rows))
    }

    fn scan_by_kind_time<'a>(
        &'a self,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let rows = self
            .inner
            .scan_by_kind_time(kinds, since, until, limit)
            .map_err(conv::store_err)?;
        Ok(Self::boxed_scan(rows))
    }

    fn query_visit(
        &self,
        query: &StoreQuery,
        limit: usize,
        visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
    ) -> Result<(), StoreError> {
        let engine_query = conv::query_into_engine(query);
        // Adapt the engine's `&StoredEngineEvent` visit to the trait's
        // `&StoredEvent` visit (one conversion per visited row, mirroring the
        // engine's per-row decode — no extra result buffer).
        let mut adapt = |se: &nmp_sqlite_wasm::StoredEngineEvent| -> ControlFlow<()> {
            visitor(&conv::stored_ref(se))
        };
        self.inner
            .query_visit(&engine_query, limit, &mut adapt)
            .map_err(conv::store_err)
    }

    fn scan_expiring_before<'a>(
        &'a self,
        unix_seconds: u64,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let rows = self
            .inner
            .scan_expiring_before(unix_seconds, limit)
            .map_err(conv::store_err)?;
        Ok(Self::boxed_scan(rows))
    }

    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError> {
        Ok(self
            .inner
            .tombstones_for(target)
            .map_err(conv::store_err)?
            .into_iter()
            .map(conv::tombstone_row)
            .collect())
    }

    fn list_tombstones<'a>(
        &'a self,
    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>
    {
        let rows = self.inner.list_tombstones().map_err(conv::store_err)?;
        Ok(Box::new(
            rows.into_iter().map(|r| Ok(conv::tombstone_row(r))),
        ))
    }

    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError> {
        Ok(self
            .inner
            .provenance_for(id)
            .map_err(conv::store_err)?
            .into_iter()
            .map(conv::provenance_entry)
            .collect())
    }

    fn list_events_seen_on(&self, relay_url: &str) -> Result<Vec<EventId>, StoreError> {
        self.inner
            .list_events_seen_on(relay_url)
            .map_err(conv::store_err)
    }

    fn relay_kind_coverage(&self, relay_url: &str) -> Result<Vec<u32>, StoreError> {
        self.inner
            .relay_kind_coverage(relay_url)
            .map_err(conv::store_err)
    }

    fn relay_kind_count(&self, relay_url: &str, kind: u32) -> Result<u64, StoreError> {
        self.inner
            .relay_kind_count(relay_url, kind)
            .map_err(conv::store_err)
    }

    // ─── Writes ───────────────────────────────────────────────────────────────

    fn insert(
        &self,
        event: VerifiedEvent,
        source: &RelayUrl,
        received_at_ms: u64,
    ) -> Result<InsertOutcome, StoreError> {
        let engine_event = conv::raw_into_engine(event.into_raw());
        let outcome = self
            .inner
            .insert(engine_event, source.as_str(), received_at_ms)
            .map_err(conv::store_err)?;
        Ok(conv::insert_outcome(outcome))
    }

    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError> {
        self.inner
            .delete_by_filter(conv::delete_filter(filter))
            .map_err(conv::store_err)
    }

    // ─── Hot-set / GC ─────────────────────────────────────────────────────────

    fn hot_set_hint(&self, ids: &[EventId]) -> Result<(), StoreError> {
        self.inner.hot_set_hint(ids).map_err(conv::store_err)
    }

    fn gc_step_with_pins(
        &self,
        budget: GcBudget,
        now_secs: u64,
        pins: &HashSet<EventId>,
    ) -> Result<GcReport, StoreError> {
        let report = self
            .inner
            .gc_step_with_pins(conv::gc_budget(budget), now_secs, pins)
            .map_err(conv::store_err)?;
        Ok(conv::gc_report(report))
    }

    fn gc_step_with_pins_and_coverage(
        &self,
        budget: GcBudget,
        now_secs: u64,
        pins: &HashSet<EventId>,
        guards: &[CoverageGuard],
    ) -> Result<GcReport, StoreError> {
        let engine_guards = conv::coverage_guards(guards);
        let report = self
            .inner
            .gc_step_with_pins_and_coverage(conv::gc_budget(budget), now_secs, pins, &engine_guards)
            .map_err(conv::store_err)?;
        Ok(conv::gc_report(report))
    }

    fn coverage_max_for_filter_hash(&self, filter_hash: &str) -> Option<u64> {
        // D6 graceful degrade: a read fault reads as "no coverage", never a panic.
        self.inner
            .coverage_max_for_filter_hash(filter_hash)
            .ok()
            .flatten()
    }

    fn coverage_rows_for_filter_hash(&self, filter_hash: &str) -> Vec<(String, u64)> {
        self.inner
            .coverage_rows_for_filter_hash(filter_hash)
            .unwrap_or_default()
    }

    // ─── Domain rows ──────────────────────────────────────────────────────────

    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle, StoreError> {
        Ok(DomainHandle {
            inner: DomainHandleInner::Opfs {
                namespace,
                backend: Arc::clone(&self.inner),
            },
        })
    }

    fn run_migrations(
        &self,
        namespace: &'static str,
        target_version: u32,
        migrations: &[DomainMigration],
    ) -> Result<(), StoreError> {
        // Reimplemented here (mirroring `mem`/`lmdb`), NOT delegated to the
        // engine's own `run_migrations`: the kernel's `DomainMigration::apply`
        // is a `fn(&mut nmp_store::MigrationTx)` — a distinct type from the
        // engine's `MigrationTx`, so the closure cannot be handed across. We run
        // the closures here against our own `MigrationTx`, stage the writes, then
        // commit them + the version bump atomically via `apply_domain_migration`.
        let current = self
            .inner
            .domain_version(namespace)
            .map_err(conv::store_err)?;
        if current > target_version {
            return Err(StoreError::SchemaTooNew {
                namespace: namespace.to_string(),
                on_disk: current,
                expected: target_version,
            });
        }
        if current == target_version {
            return Ok(());
        }

        let mut staged: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for m in migrations {
            if m.from_version < current || m.from_version >= target_version {
                continue;
            }
            let mut tx = MigrationTx::default();
            (m.apply)(&mut tx).map_err(|reason| StoreError::MigrationFailed {
                namespace: namespace.to_string(),
                from: m.from_version,
                to: m.to_version,
                reason,
            })?;
            staged.extend(tx.writes().iter().cloned());
        }

        self.inner
            .apply_domain_migration(namespace, target_version, &staged)
            .map_err(conv::store_err)
    }

    // ─── F-TTL replaceable freshness ────────────────────────────────────────────

    fn get_check_again_after(&self, key: &ReplaceableKey) -> Option<u64> {
        self.inner
            .get_check_again_after(&conv::replaceable_key(key))
            .ok()
            .flatten()
    }

    fn set_check_again_after(&self, key: ReplaceableKey, ts_ms: u64) {
        // D6 graceful degrade: a stamp fault must never block ingest/claim.
        let _ = self
            .inner
            .set_check_again_after(&conv::replaceable_key(&key), ts_ms);
    }

    // ─── K3 coverage ledger ─────────────────────────────────────────────────────

    fn record_coverage(&self, filter_hash: &str, relay: &str, covered_through: u64) {
        // D6 graceful degrade: a missed coverage write only loses a since-floor
        // hint for this shape, never a wrong answer.
        let _ = self
            .inner
            .record_coverage(filter_hash, relay, covered_through);
    }

    fn get_coverage(&self, filter_hash: &str, relay: &str) -> Option<u64> {
        self.inner.get_coverage(filter_hash, relay).ok().flatten()
    }

    // ─── Ingest log ─────────────────────────────────────────────────────────────

    fn latest_ingest_seq(&self) -> Result<u64, StoreError> {
        self.inner.latest_ingest_seq().map_err(conv::store_err)
    }

    fn oldest_available_seq(&self) -> Result<Option<u64>, StoreError> {
        self.inner.oldest_available_seq().map_err(conv::store_err)
    }

    fn scan_log_since_seq(
        &self,
        after_seq: u64,
        limit: usize,
    ) -> Result<ScanLogResult, StoreError> {
        Ok(conv::scan_log_result(
            self.inner
                .scan_log_since_seq(after_seq, limit)
                .map_err(conv::store_err)?,
        ))
    }

    fn replace_log_retention_claims(&self, claims: &[LogRetentionClaim]) {
        // VOLATILE wholesale replace (ADR-0058 §6). A fault is non-fatal: the
        // next append-time trim uses the prior set; the consumer degrades to an
        // explicit PullGap, never a silent skip.
        let _ = self
            .inner
            .replace_log_retention_claims(&conv::retention_claims(claims));
    }

    // ─── Export ─────────────────────────────────────────────────────────────────

    fn dump(
        &self,
        out: &mut dyn std::io::Write,
        format: DumpFormat,
    ) -> Result<DumpStats, StoreError> {
        match format {
            DumpFormat::Jsonl => {
                // `&mut dyn Write` implements `Write` via the std blanket impl, so
                // it satisfies the engine's `&mut impl Write` JSONL sink.
                let mut sink = out;
                let stats = self.inner.dump(&mut sink).map_err(conv::store_err)?;
                Ok(conv::dump_stats(stats))
            }
        }
    }

    // ─── Full-text search ────────────────────────────────────────────────────────
    //
    // The OPFS-SQLite engine ships no FTS index yet, so the trait defaults apply:
    // `install_search_index_specs` no-ops, `cache_search_scopes` is empty, and
    // `text_search_visit` returns `TextSearchStatus::Unsupported` (identical to a
    // non-FTS backend). Search shapes stay relay-served until a later PR adds the
    // engine-side inverted index — exactly the LMDB/Mem default contract.
}
