//! Event storage abstraction for NMP.
//!
//! # Crate placement
//!
//! The store lives in `nmp-core::store` (not a separate crate) because the
//! `EventStore::run_migrations` method references `crate::substrate::DomainMigration`.
//! A separate crate would create a circular dependency:
//!   `nmp-store` → `nmp-core` (for DomainMigration) → `nmp-store`
//!
//! # Backends
//!
//! - `MemEventStore`: always compiled; used in tests and pre-M15 web builds.
//! - `LmdbEventStore`: compiled always but only functional with `--features lmdb-backend`.
//!
//! See `docs/design/lmdb/trait.md` for the full design specification.

mod events;
mod lmdb;
mod mem;
pub mod types;

pub use events::{DomainHandle, DomainScanIter, EventIter, EventStore};
pub use lmdb::LmdbEventStore;
pub use mem::MemEventStore;
pub use types::{
    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
    InsertOutcome, ProvenanceEntry, PubKey, RawEvent, RejectReason, RelayUrl, StoreQuery,
    StoredEvent, SyncMethod, TombstoneOrigin, TombstoneRow, VerifiedEvent, WatermarkKey,
    WatermarkRow,
};

// Re-export error types from types (defined there to avoid circular imports).
pub use types::{StoreError, VerifyError};

use std::path::PathBuf;

/// Storage backend selector.
#[derive(Clone, Debug)]
pub enum StorageBackend {
    Memory,
    Lmdb { path: PathBuf },
}

/// Factory: construct a `Box<dyn EventStore>` from a backend selector.
pub fn open_event_store(
    backend: &StorageBackend,
) -> Result<Box<dyn EventStore>, StoreError> {
    match backend {
        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
        StorageBackend::Lmdb { path } => Ok(Box::new(LmdbEventStore::open(path)?)),
    }
}
