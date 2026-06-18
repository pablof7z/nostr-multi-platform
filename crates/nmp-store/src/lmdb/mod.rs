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
mod query_streaming;
#[cfg(feature = "lmdb-backend")]
mod query_relay_index;
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

#[cfg(feature = "lmdb-backend")]
mod inner {
    use std::sync::atomic::AtomicU64;

    use heed::types::Bytes;
    use heed::{Database, Env};
    use nmp_nostr_lmdb::Lmdb;

    /// Internal storage handles shared by every method.
    ///
    /// The `Env` is owned by both `Lmdb` (which opened the upstream 11 dbs on
    /// it) and by this struct's sub-db handles. The `Lmdb` clone holds its own
    /// `Env` clone — heed's `Env` is internally ref-counted so this is cheap.
    pub struct Inner {
        pub(crate) env: Env,
        pub(crate) lmdb: Lmdb,
        /// Per-id provenance: event_id (32 bytes) → bincode(Vec<ProvenanceEntry>).
        pub(crate) provenance: Database<Bytes, Bytes>,
        /// Per-id tombstones with full metadata (NMP-side).
        /// Key: target_id (32 bytes). Value: bincode(TombstoneRow).
        pub(crate) tombstones: Database<Bytes, Bytes>,
        /// Address tombstones for param-replaceable kinds.
        /// Key: "kind:pk_hex:dtag" bytes. Value: bincode(TombstoneRow).
        pub(crate) addr_tombstones: Database<Bytes, Bytes>,
        /// Domain schema versions: namespace bytes → u32 BE.
        pub(crate) domain_versions: Database<Bytes, Bytes>,
        /// Domain data: namespace bytes || 0x00 || key bytes → value bytes.
        pub(crate) domain_data: Database<Bytes, Bytes>,
        /// Relay-author scores: `[32 pubkey bytes][1 url-len u8][N url bytes]` →
        /// `[u32 successes BE][u32 failures BE][u64 last_used_unix_s BE][u64 reserved BE]`.
        /// See `relay_scores.rs` for the encode/decode layer and canonicalization.
        pub(crate) relay_author_scores: Database<Bytes, Bytes>,
        /// V-60 LRU access index: event_id (32 bytes) → seq (8 bytes BE).
        ///
        /// Stamped on insert and on every `get_by_id` hit.  Used by `gc_step`
        /// to identify the least-recently-accessed un-pinned events for eviction.
        ///
        /// Design trade-off: stamping on read converts a read-txn into a write-txn
        /// on `get_by_id`.  We accept this cost only for point-reads (not bulk
        /// scans) to bound write-amplification.  The alternative (wall-clock in
        /// a read-txn) would reintroduce a D7 violation; using a persisted
        /// monotonic counter is the only D7-safe option for LMDB.
        pub(crate) lru_access: Database<Bytes, Bytes>,
        /// Monotonic sequence counter for LRU ordering.  Initialised from
        /// `max(lru_access values) + 1` on open so a crash-restart doesn't
        /// reuse seqs that are already in the db.  `Relaxed` ordering is fine —
        /// each store op holds the `heed::RwTxn` lock so there is no concurrent
        /// writer.
        pub(crate) lru_seq: AtomicU64,

        /// V-118 expiration index: expiry_ts_be(8) || event_id(32) → empty.
        ///
        /// Key encoding: expiry timestamp as 8-byte big-endian u64 (NOT inverted,
        /// so lower timestamps sort first — range scan for entries ≤ now_secs is
        /// a simple prefix scan up to `[now_secs+1; 0..0]`).
        ///
        /// Maintained by: insert (if event has expiration tag), all delete paths.
        /// Backfilled on store open for existing databases (V-118).
        ///
        /// This replaces the V-117 `gc_phase1_cursor` heuristic: Phase 1 now does
        /// an O(expired) range scan on this index rather than an O(store) scan of
        /// all events, eliminating the same-`created_at`-block stall (#1097).
        pub(crate) expiry_index: Database<Bytes, Bytes>,

        /// V-52 relay-origin reverse index: `relay_url || 0x00 || event_id(32)`
        /// → empty (presence-only).
        ///
        /// Mirrors the `MemEventStore::relay_index` so `list_events_seen_on` is an
        /// O(events-on-relay) prefix range scan on LMDB instead of an O(store)
        /// provenance scan.  The `0x00` separator is safe: relay URLs are valid
        /// UTF-8 (`wss://…`) and never contain a NUL byte, so the separator can
        /// never appear inside the `relay_url` segment — every key is therefore
        /// unambiguously decodable as `(relay_url, event_id)`.
        ///
        /// Maintained by: the provenance write path (`provenance::upsert` adds the
        /// `(relay, id)` entry; `provenance::delete` removes every `(relay, id)`
        /// entry for the id by first reading the event's provenance relays).
        /// Backfilled once on store open for pre-V-52 databases — see
        /// `open.rs::backfill_relay_index`.
        pub(crate) relay_index: Database<Bytes, Bytes>,

        /// #1518 relay×kind presence index (`nmp-relay-kind`): `relay_url ||
        /// 0x00 || kind(BE4) || event_id(32)` → empty (presence-only).
        ///
        /// A derived projection of per-event provenance committed in the SAME
        /// `RwTxn` as every provenance write — it tells the kernel which kinds
        /// (and how many events of each) a relay has actually served, without
        /// splitting store ownership.  Privacy-gated: NIP-04/17/59 kinds
        /// (4/13/14/15/1059/1060) never enter the index (checked at write time
        /// in `provenance::relay_kind_put`).
        ///
        /// Maintained by the same provenance write path as `relay_index`
        /// (`provenance::upsert` / `provenance::delete`).  Backfilled once on
        /// open for pre-#1518 databases — see `open.rs::backfill_relay_kind_index`.
        pub(crate) relay_kind: Database<Bytes, Bytes>,

        /// K3 coverage ledger (ADR-0056 §3, Stage D1): `filter_hash || 0x1F ||
        /// relay_url` → `covered_through` (8-byte BE unix-seconds).
        ///
        /// Records, per `(filter_hash, relay)`, the downward-closed timestamp
        /// through which a sync has COMPLETED (EOSE on an un-floored REQ, or
        /// NEG-DONE). Written by `EventStore::record_coverage`, read by
        /// `EventStore::get_coverage` — the kernel's since-floor source. This is
        /// the purpose-built successor to the #1090-deleted `nmp-watermarks`
        /// sub-db — re-created with real readers/writers, not re-activated
        /// (ADR-0056 §2.1).
        pub(crate) coverage: Database<Bytes, Bytes>,

        /// Interaction-counter sidecar (issue #1519).
        ///
        /// Key: `target_event_id(32) || counter_kind(1)`.
        /// Value: `count(8 bytes, big-endian u64)`.
        ///
        /// Written atomically with event inserts and removes — same `RwTxn`
        /// (ADR-0011). Counts are always consistent with the stored event set.
        pub(crate) interaction_counters: Database<Bytes, Bytes>,

        /// True when the `nmp-interaction-counters` sub-db schema version is
        /// known (version == 1). Set false if an unknown future version is
        /// detected on open, causing reads to fall back to
        /// `TargetInteractionCounts::default()` (forward-compat safeguard).
        pub(crate) interaction_counters_usable: bool,

        // ── GC scan state (V-117 fixes) ───────────────────────────────────────
        /// Phase-3/3b tombstone-purge gate: unix_secs of the last pass that
        /// actually ran the tombstone scan.  The Phase-3 scan iterates and
        /// serde-decodes every tombstone row inside a WRITE txn — budget-bound
        /// on count but still O(tombstones) per call.  Throttle to at most once
        /// per `GC_TOMBSTONE_PURGE_INTERVAL_SECS` (1 h) so the 60-second gc tick
        /// does not iterate the full tombstone db every minute.
        ///
        /// Value 0 means "never run" (correct: a 90-day tombstone age threshold
        /// means nothing is purgeable in the first hour anyway).
        pub(crate) gc_last_tombstone_purge_secs: AtomicU64,
    }

    impl std::fmt::Debug for Inner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Inner").field("lmdb", &"<Lmdb>").finish()
        }
    }
}

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
}
