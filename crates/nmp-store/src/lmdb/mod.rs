//! LMDB `EventStore` backend.
//!
//! Architecture (ADR-0011 + ADR-0012): a single `heed::Env` is owned by NMP
//! and injected into the `nmp-nostr-lmdb` fork via `Lmdb::with_env`. This lets
//! NMP open its own sub-databases on the same env so that every `insert()`
//! commits the event write + NMP secondaries inside one `RwTxn` — atomicity
//! across the whole pipeline.
//!
//! See:
//!   * `docs/decisions/0011-lmdb-env-sharing.md` — env-sharing policy
//!   * `docs/decisions/0012-lmdb-write-path-policy.md` — write-path semantics
//!
//! When compiled without `--features lmdb-backend` this module exposes only
//! the `LmdbEventStore` newtype and an `open()` that returns
//! `StoreError::Io("lmdb-backend not enabled")`. Every trait method is the
//! same stub. Tests for the LMDB backend live in `tests.rs` and are
//! `#[cfg(feature = "lmdb-backend")]` gated.

#[cfg(feature = "lmdb-backend")]
mod conv;
// K3 Stage D1 (ADR-0056 §3) — coverage-ledger read/write helpers.
#[cfg(feature = "lmdb-backend")]
mod coverage;
#[cfg(feature = "lmdb-backend")]
mod delete;
#[cfg(feature = "lmdb-backend")]
pub(crate) mod domain;
#[cfg(feature = "lmdb-backend")]
mod dump;
#[cfg(feature = "lmdb-backend")]
mod gc;
// Phase 3+3b tombstone purge extracted from gc.rs for the 500-LOC hard cap.
#[cfg(feature = "lmdb-backend")]
mod gc_tombstones;
// Secondary-index maintenance primitives (LRU / expiry-index / freshness-key)
// extracted from gc.rs for the 500-LOC cap; re-exported via `gc::`.
#[cfg(feature = "lmdb-backend")]
mod gc_index;
#[cfg(feature = "lmdb-backend")]
mod insert;
// NIP-09 (kind:5) deletion handling extracted from insert.rs for the LOC cap.
#[cfg(feature = "lmdb-backend")]
mod insert_kind5;
#[cfg(feature = "lmdb-backend")]
mod provenance;
#[cfg(feature = "lmdb-backend")]
mod query;
// Streaming helpers extracted from query.rs to stay within the 500-LOC gate.
#[cfg(feature = "lmdb-backend")]
mod query_relay_index;
#[cfg(feature = "lmdb-backend")]
mod query_streaming;
#[cfg(feature = "lmdb-backend")]
mod store_impl;
#[cfg(feature = "lmdb-backend")]
mod tombstones;
// W2 — relay-author-scores LMDB encode/decode layer.
#[cfg(feature = "lmdb-backend")]
pub mod relay_scores;
// Issue #1519 — interaction-counter sidecar.
#[cfg(feature = "lmdb-backend")]
mod interaction_counters;
// Sub-db + env open logic extracted for LOC budget.
#[cfg(feature = "lmdb-backend")]
mod open;
// LMDB-error classifier: heed/MdbError → typed StoreError variants (#1521).
#[cfg(feature = "lmdb-backend")]
mod open_error;
// ADR-0058 §4 — ingest-log LMDB helpers.
#[cfg(feature = "lmdb-backend")]
mod ingest_log;

#[cfg(all(test, feature = "lmdb-backend"))]
mod test_fixtures;
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests;
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_kind5;
// AuthorsKind multi-author query parity (split from tests.rs for 500-LOC cap).
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_authors_kind;
// addr-tombstone GC tests — S-2 audit fix (split from tests.rs for 500-LOC cap).
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_addr_tombstone;
// W2 TDD gate-tests for `relay_scores`.
#[cfg(all(test, feature = "lmdb-backend"))]
mod relay_scores_tests;
// V-117 GC budget / resumable-cursor / tombstone-gate tests.
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_gc;
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_gc_bulk_delete; // test 10 (bulk delete expiry-index cleanup).
                          // #1090 Stage-1 — derived pin set for gc_step.
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_gc_stage1;
// V-52 (#969) — relay-origin reverse index parity + persistence + backfill.
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_relay_index;
// #1518 — relay×kind presence index parity + persistence + backfill.
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_relay_kind;
// #1480 — production budget keeps durable LRU disabled; finite retention is explicit.
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_gc_stage3;
// Secondary-index integrity tests (Bug-1: kind:5 a-tag leaks; Bug-2: freshness leaks).
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_secondary_index;
// Issue #1519 — interaction-counter sidecar tests.
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_interaction_counters;
// #1521 — typed LMDB health diagnostics: classifier unit tests + integration tests.
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_health_diag;
// ADR-0058 step-1 — ingest-log smoke tests.
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_ingest_log;
// ADR-0058 fix-verification (split for 500-LOC cap).
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_ingest_log_fixes;
// ADR-0058 §6 step-4 — Protected-cursor log-retention tests.
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_retention;

use std::path::{Path, PathBuf};

use super::StoreError;

#[cfg(not(feature = "lmdb-backend"))]
use super::events::{DomainHandle, EventIter, EventStore};
#[cfg(not(feature = "lmdb-backend"))]
use super::types::{
    DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport, InsertOutcome,
    ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, TombstoneRow, VerifiedEvent,
};
#[cfg(not(feature = "lmdb-backend"))]
use crate::DomainMigration;
#[cfg(not(feature = "lmdb-backend"))]
use std::collections::HashSet;
#[cfg(not(feature = "lmdb-backend"))]
use std::ops::ControlFlow;

// ─── Test-support counter re-exports ─────────────────────────────────────────
//
// `nmp-testing` integration tests consume these through `nmp_store::lmdb::*`
// when built with `--features test-support,lmdb-backend`.

#[cfg(all(feature = "lmdb-backend", any(test, feature = "test-support")))]
pub use query::{conversion_count, reset_conversion_count};

// ─── Internal sub-db / env handles (feature-on only) ─────────────────────────

#[cfg(feature = "lmdb-backend")]
pub(crate) use inner::Inner;

// Internal sub-db / env handles extracted to inner.rs for the 500-LOC cap.
#[cfg(feature = "lmdb-backend")]
mod inner;

// ─── LmdbEventStore ──────────────────────────────────────────────────────────

/// Production LMDB-backed `EventStore`.
#[derive(Clone)]
pub struct LmdbEventStore {
    #[allow(dead_code)] // path retained for diagnostics + future re-open.
    path: PathBuf,
    #[cfg(feature = "lmdb-backend")]
    inner: std::sync::Arc<Inner>,
}

impl LmdbEventStore {
    /// Open or create an LMDB store at `path`.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        #[cfg(feature = "lmdb-backend")]
        {
            open::open_impl(path)
        }
        #[cfg(not(feature = "lmdb-backend"))]
        {
            let _ = path;
            Err(StoreError::Io(
                "lmdb-backend feature not enabled — recompile with --features lmdb-backend".into(),
            ))
        }
    }

    #[cfg(not(feature = "lmdb-backend"))]
    fn not_enabled() -> StoreError {
        StoreError::Io("lmdb-backend feature not enabled".into())
    }

    /// Test-only: expose the inner Arc so tests can directly inspect sub-dbs.
    ///
    /// Only compiled under `#[cfg(test)]` — not part of the public API.
    #[cfg(all(test, feature = "lmdb-backend"))]
    pub(crate) fn inner_for_test(&self) -> &std::sync::Arc<Inner> {
        &self.inner
    }
}

// ─── Feature-off stub trait impl ─────────────────────────────────────────────
//
// When the lmdb-backend feature is OFF, every method returns the not_enabled
// error. The feature-on implementations live in store_impl.rs (delegating
// through the per-subsystem modules).

#[cfg(not(feature = "lmdb-backend"))]
impl EventStore for LmdbEventStore {
    fn get_by_id(&self, _id: &EventId) -> Result<Option<StoredEvent>, StoreError> {
        Err(Self::not_enabled())
    }
    fn scan_by_author_kind<'a>(
        &'a self,
        _author: &PubKey,
        _kinds: &[u32],
        _since: Option<u64>,
        _until: Option<u64>,
        _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        Err(Self::not_enabled())
    }
    fn scan_by_authors_kind<'a>(
        &'a self,
        _authors: &std::collections::BTreeSet<PubKey>,
        _kinds: &[u32],
        _since: Option<u64>,
        _until: Option<u64>,
        _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        Err(Self::not_enabled())
    }
    fn get_param_replaceable(
        &self,
        _pubkey: &PubKey,
        _kind: u32,
        _d_tag: &[u8],
    ) -> Result<Option<StoredEvent>, StoreError> {
        Err(Self::not_enabled())
    }
    fn scan_by_kind_dtag<'a>(
        &'a self,
        _kind: u32,
        _d_tag: &[u8],
        _since: Option<u64>,
        _until: Option<u64>,
        _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        Err(Self::not_enabled())
    }
    fn scan_by_etag<'a>(
        &'a self,
        _target: &EventId,
        _kinds: &[u32],
        _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        Err(Self::not_enabled())
    }
    fn scan_by_ptag<'a>(
        &'a self,
        _target: &PubKey,
        _kinds: &[u32],
        _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        Err(Self::not_enabled())
    }
    fn scan_by_kind_time<'a>(
        &'a self,
        _kinds: &[u32],
        _since: Option<u64>,
        _until: Option<u64>,
        _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        Err(Self::not_enabled())
    }
    fn scan_expiring_before<'a>(
        &'a self,
        _unix_seconds: u64,
        _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        Err(Self::not_enabled())
    }
    fn tombstones_for(&self, _target: &EventId) -> Result<Vec<TombstoneRow>, StoreError> {
        Err(Self::not_enabled())
    }
    fn list_tombstones<'a>(
        &'a self,
    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>
    {
        Err(Self::not_enabled())
    }
    fn provenance_for(&self, _id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError> {
        Err(Self::not_enabled())
    }
    fn list_events_seen_on(&self, _relay_url: &str) -> Result<Vec<EventId>, StoreError> {
        Err(Self::not_enabled())
    }
    fn relay_kind_coverage(&self, _relay_url: &str) -> Result<Vec<u32>, StoreError> {
        Err(Self::not_enabled())
    }
    fn relay_kind_count(&self, _relay_url: &str, _kind: u32) -> Result<u64, StoreError> {
        Err(Self::not_enabled())
    }
    fn insert(
        &self,
        _event: VerifiedEvent,
        _source: &RelayUrl,
        _received_at_ms: u64,
    ) -> Result<InsertOutcome, StoreError> {
        Err(Self::not_enabled())
    }
    fn delete_by_filter(&self, _filter: DeleteFilter) -> Result<usize, StoreError> {
        Err(Self::not_enabled())
    }
    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
        Ok(())
    }
    fn gc_step_with_pins(
        &self,
        _budget: GcBudget,
        _now_secs: u64,
        _pins: &HashSet<EventId>,
    ) -> Result<GcReport, StoreError> {
        Err(Self::not_enabled())
    }
    fn domain_open(&self, _namespace: &'static str) -> Result<DomainHandle, StoreError> {
        Err(Self::not_enabled())
    }
    fn run_migrations(
        &self,
        _namespace: &'static str,
        _target_version: u32,
        _migrations: &[DomainMigration],
    ) -> Result<(), StoreError> {
        Err(Self::not_enabled())
    }
    fn dump(
        &self,
        _out: &mut dyn std::io::Write,
        _format: DumpFormat,
    ) -> Result<DumpStats, StoreError> {
        Err(Self::not_enabled())
    }
    fn query_visit(
        &self,
        _query: &StoreQuery,
        _limit: usize,
        _visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
    ) -> Result<(), StoreError> {
        Err(Self::not_enabled())
    }
    fn latest_ingest_seq(&self) -> Result<u64, StoreError> {
        Err(Self::not_enabled())
    }
    fn oldest_available_seq(&self) -> Result<Option<u64>, StoreError> {
        Err(Self::not_enabled())
    }
    fn scan_log_since_seq(
        &self,
        _after_seq: u64,
        _limit: usize,
    ) -> Result<crate::ingest_log::ScanLogResult, StoreError> {
        Err(Self::not_enabled())
    }
}
