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
#[cfg(feature = "lmdb-backend")]
mod delete;
#[cfg(feature = "lmdb-backend")]
mod insert;
#[cfg(feature = "lmdb-backend")]
mod query;
#[cfg(feature = "lmdb-backend")]
mod provenance;
#[cfg(feature = "lmdb-backend")]
mod tombstones;
#[cfg(feature = "lmdb-backend")]
mod claims;
#[cfg(feature = "lmdb-backend")]
mod gc;
#[cfg(feature = "lmdb-backend")]
pub(crate) mod domain;
#[cfg(feature = "lmdb-backend")]
mod dump;
#[cfg(feature = "lmdb-backend")]
mod store_impl;
// W2 — relay-author-scores LMDB encode/decode layer.
#[cfg(feature = "lmdb-backend")]
pub mod relay_scores;

#[cfg(all(test, feature = "lmdb-backend"))]
mod test_fixtures;
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests;
#[cfg(all(test, feature = "lmdb-backend"))]
mod tests_kind5;
// W2 TDD gate-tests for `relay_scores`.
#[cfg(all(test, feature = "lmdb-backend"))]
mod relay_scores_tests;

use std::path::{Path, PathBuf};

use super::StoreError;

#[cfg(not(feature = "lmdb-backend"))]
use std::ops::ControlFlow;
#[cfg(not(feature = "lmdb-backend"))]
use super::events::{DomainHandle, EventIter, EventStore};
#[cfg(not(feature = "lmdb-backend"))]
use super::types::{
    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
    InsertOutcome, ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, TombstoneRow,
    VerifiedEvent, WatermarkKey, WatermarkRow,
};
#[cfg(not(feature = "lmdb-backend"))]
use crate::DomainMigration;

// ─── Internal sub-db / env handles (feature-on only) ─────────────────────────

#[cfg(feature = "lmdb-backend")]
pub(crate) use inner::Inner;

#[cfg(feature = "lmdb-backend")]
mod inner {
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
        /// Watermarks: filter_hash(32) || relay_url bytes → bincode(WatermarkRow).
        pub(crate) watermarks: Database<Bytes, Bytes>,
        /// Claim budgets: claimer_u64 (8 bytes BE) → usize (8 bytes BE).
        pub(crate) claims_budget: Database<Bytes, Bytes>,
        /// Claims: claimer_u64 (8 bytes BE) || event_id(32) → empty value.
        pub(crate) claims: Database<Bytes, Bytes>,
        /// Domain schema versions: namespace bytes → u32 BE.
        pub(crate) domain_versions: Database<Bytes, Bytes>,
        /// Domain data: namespace bytes || 0x00 || key bytes → value bytes.
        pub(crate) domain_data: Database<Bytes, Bytes>,
        /// W2 — relay-author-scores: `[32 pubkey bytes][1 url-len u8][N url bytes]` →
        /// `[u32 successes BE][u32 failures BE][u64 last_used_unix_s BE][u64 reserved BE]`.
        /// See `relay_scores.rs` for the encode/decode layer and §8.9/§8.10 of the impl plan.
        pub(crate) relay_author_scores: Database<Bytes, Bytes>,
    }

    impl std::fmt::Debug for Inner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Inner").field("lmdb", &"<Lmdb>").finish()
        }
    }
}

// ─── LmdbEventStore ──────────────────────────────────────────────────────────

/// Production LMDB-backed `EventStore`.
pub struct LmdbEventStore {
    #[allow(dead_code)] // path retained for diagnostics + future re-open.
    path: PathBuf,
    #[cfg(feature = "lmdb-backend")]
    inner: std::sync::Arc<Inner>,
}

impl LmdbEventStore {
    /// Open or create an LMDB store at `path`.
    #[must_use]
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        #[cfg(feature = "lmdb-backend")]
        {
            open_impl(path)
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
}

#[cfg(feature = "lmdb-backend")]
fn open_impl(path: &Path) -> Result<LmdbEventStore, StoreError> {
    use heed::types::Bytes;
    use nmp_nostr_lmdb::Lmdb;
    use std::sync::Arc;

    // 32 GB on 64-bit; the upstream default. The fork's `with_env` wraps the
    // 11 internal sub-dbs; we reserve 8 additional for NMP-side data.
    const MAP_SIZE: usize = 1024 * 1024 * 1024 * 32;
    const MAX_READERS: u32 = 126;
    const NMP_ADDITIONAL_DBS: u32 = 9; // W2: +1 for relay-author-scores-v1

    std::fs::create_dir_all(path).map_err(|e| StoreError::Io(e.to_string()))?;

    let env = Lmdb::open_env(path, MAP_SIZE, MAX_READERS, NMP_ADDITIONAL_DBS)
        .map_err(|e| StoreError::Io(format!("open_env: {e}")))?;
    let lmdb = Lmdb::with_env(env.clone())
        .map_err(|e| StoreError::Io(format!("with_env: {e}")))?;

    // Open NMP sub-dbs on the shared env.
    let mut txn = env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    let open = |name: &str, txn: &mut heed::RwTxn| -> Result<heed::Database<Bytes, Bytes>, StoreError> {
        env.database_options()
            .types::<Bytes, Bytes>()
            .name(name)
            .create(txn)
            .map_err(|e| StoreError::Io(format!("open {name}: {e}")))
    };
    let provenance = open("nmp-provenance", &mut txn)?;
    let tombstones = open("nmp-tombstones", &mut txn)?;
    let addr_tombstones = open("nmp-addr-tombstones", &mut txn)?;
    let watermarks = open("nmp-watermarks", &mut txn)?;
    let claims_budget = open("nmp-claims-budget", &mut txn)?;
    let claims = open("nmp-claims", &mut txn)?;
    let domain_versions = open("nmp-domain-versions", &mut txn)?;
    let domain_data = open("nmp-domain-data", &mut txn)?;
    // W2 — relay-author-scores sub-db.
    let relay_author_scores = open(relay_scores::SUB_DB_NAME, &mut txn)?;
    txn.commit()
        .map_err(|e| StoreError::Io(format!("commit init: {e}")))?;

    Ok(LmdbEventStore {
        path: path.to_path_buf(),
        inner: Arc::new(Inner {
            env,
            lmdb,
            provenance,
            tombstones,
            addr_tombstones,
            watermarks,
            claims_budget,
            claims,
            domain_versions,
            domain_data,
            relay_author_scores,
        }),
    })
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
        &'a self, _author: &PubKey, _kinds: &[u32], _since: Option<u64>,
        _until: Option<u64>, _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> { Err(Self::not_enabled()) }
    fn get_param_replaceable(
        &self, _pubkey: &PubKey, _kind: u32, _d_tag: &[u8],
    ) -> Result<Option<StoredEvent>, StoreError> { Err(Self::not_enabled()) }
    fn scan_by_kind_dtag<'a>(
        &'a self, _kind: u32, _d_tag: &[u8], _since: Option<u64>,
        _until: Option<u64>, _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> { Err(Self::not_enabled()) }
    fn scan_by_etag<'a>(
        &'a self, _target: &EventId, _kinds: &[u32], _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> { Err(Self::not_enabled()) }
    fn scan_by_ptag<'a>(
        &'a self, _target: &PubKey, _kinds: &[u32], _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> { Err(Self::not_enabled()) }
    fn scan_by_kind_time<'a>(
        &'a self, _kinds: &[u32], _since: Option<u64>, _until: Option<u64>, _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> { Err(Self::not_enabled()) }
    fn scan_expiring_before<'a>(
        &'a self, _unix_seconds: u64, _limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> { Err(Self::not_enabled()) }
    fn tombstones_for(&self, _target: &EventId) -> Result<Vec<TombstoneRow>, StoreError> {
        Err(Self::not_enabled())
    }
    fn list_tombstones<'a>(
        &'a self,
    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>
    { Err(Self::not_enabled()) }
    fn provenance_for(&self, _id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError> {
        Err(Self::not_enabled())
    }
    fn insert(
        &self, _event: VerifiedEvent, _source: &RelayUrl, _received_at_ms: u64,
    ) -> Result<InsertOutcome, StoreError> { Err(Self::not_enabled()) }
    fn delete_by_filter(&self, _filter: DeleteFilter) -> Result<usize, StoreError> {
        Err(Self::not_enabled())
    }
    fn read_watermark(&self, _key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError> {
        Err(Self::not_enabled())
    }
    fn write_watermark(&self, _row: WatermarkRow) -> Result<(), StoreError> {
        Err(Self::not_enabled())
    }
    fn coverage(&self, _key: &WatermarkKey) -> Result<Coverage, StoreError> {
        Err(Self::not_enabled())
    }
    fn list_watermarks_for_relay<'a>(
        &'a self, _relay_url: &str,
    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
    { Err(Self::not_enabled()) }
    fn register_view_cover(
        &self, _claimer: ClaimerId, _cover_budget: usize,
    ) -> Result<(), StoreError> { Err(Self::not_enabled()) }
    fn claim(&self, _claimer: ClaimerId, _ids: &[EventId]) -> Result<(), StoreError> {
        Err(Self::not_enabled())
    }
    fn release(&self, _claimer: ClaimerId) -> Result<(), StoreError> {
        Err(Self::not_enabled())
    }
    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> { Ok(()) }
    fn gc_step(&self, _budget: GcBudget) -> Result<GcReport, StoreError> {
        Err(Self::not_enabled())
    }
    fn domain_open(&self, _namespace: &'static str) -> Result<DomainHandle, StoreError> {
        Err(Self::not_enabled())
    }
    fn run_migrations(
        &self, _namespace: &'static str, _target_version: u32,
        _migrations: &[DomainMigration],
    ) -> Result<(), StoreError> { Err(Self::not_enabled()) }
    fn dump(
        &self, _out: &mut dyn std::io::Write, _format: DumpFormat,
    ) -> Result<DumpStats, StoreError> { Err(Self::not_enabled()) }
    fn query_visit(
        &self,
        _query: &StoreQuery,
        _limit: usize,
        _visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
    ) -> Result<(), StoreError> { Err(Self::not_enabled()) }
}
