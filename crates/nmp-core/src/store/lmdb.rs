//! LMDB `EventStore` backend — skeleton for M3 phase 2.
//!
//! All methods return `StoreError::Io("lmdb-backend feature not enabled")` when
//! compiled without the `lmdb-backend` feature (which is the case in this M3
//! phase-1 deliverable). The full implementation is deferred to the follow-up
//! M3-lmdb task.
//!
//! See `docs/design/lmdb/trait.md` §5 and `docs/decisions/0011-lmdb-env-sharing.md`.

use std::path::{Path, PathBuf};

use super::events::{DomainHandle, EventIter, EventStore};
use super::types::{
    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
    InsertOutcome, ProvenanceEntry, PubKey, RawEvent, RelayUrl, StoredEvent, TombstoneRow,
    WatermarkKey, WatermarkRow,
};
use super::StoreError;
use crate::substrate::DomainMigration;

/// Production LMDB-backed `EventStore`.
///
/// Architecture (ADR-0011): two separate heed environments.
/// - `path/nostr/` — owned by `nostr-lmdb` (upstream crate, eventual integration).
/// - `path/nmp/`   — owned by NMP; holds watermarks, domain rows, provenance, GC metadata.
///
/// Atomicity across the two environments is best-effort with startup repair.
pub struct LmdbEventStore {
    #[allow(dead_code)] // Used by lmdb-backend feature; skeleton in M3 phase 1.
    path: PathBuf,
    _private: (),
}

impl LmdbEventStore {
    /// Open or create an LMDB store at `path`.
    ///
    /// Creates `path/nostr/` and `path/nmp/` subdirectories.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        #[cfg(feature = "lmdb-backend")]
        {
            let nostr_dir = path.join("nostr");
            let nmp_dir = path.join("nmp");
            std::fs::create_dir_all(&nostr_dir)
                .map_err(|e| StoreError::Io(e.to_string()))?;
            std::fs::create_dir_all(&nmp_dir)
                .map_err(|e| StoreError::Io(e.to_string()))?;
            return Ok(Self { path: path.to_owned(), _private: () });
        }

        // Without lmdb-backend: store the path for future use, then fail.
        let _ = path;
        Err(StoreError::Io(
            "lmdb-backend feature not enabled — recompile with --features lmdb-backend".into(),
        ))
    }

    fn not_enabled() -> StoreError {
        StoreError::Io("lmdb-backend feature not enabled".into())
    }
}

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

    fn insert(
        &self,
        _event: RawEvent,
        _source: &RelayUrl,
        _received_at_ms: u64,
    ) -> Result<InsertOutcome, StoreError> {
        Err(Self::not_enabled())
    }

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
        &'a self,
        _relay_url: &str,
    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
    {
        Err(Self::not_enabled())
    }

    fn register_view_cover(
        &self,
        _claimer: ClaimerId,
        _cover_budget: usize,
    ) -> Result<(), StoreError> {
        Err(Self::not_enabled())
    }

    fn claim(&self, _claimer: ClaimerId, _ids: &[EventId]) -> Result<(), StoreError> {
        Err(Self::not_enabled())
    }

    fn release(&self, _claimer: ClaimerId) -> Result<(), StoreError> {
        Err(Self::not_enabled())
    }

    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
        Err(Self::not_enabled())
    }

    fn gc_step(&self, _budget: GcBudget) -> Result<GcReport, StoreError> {
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
}
