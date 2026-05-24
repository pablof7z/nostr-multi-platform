//! Event storage abstraction for NMP.
//!
//! # Crate placement
//!
//! Extracted from `nmp-core::store` (step 9 of the crate-boundary migration —
//! see `docs/architecture/crate-boundaries.md` §5). `nmp-core` re-exports the
//! public surface as `nmp_core::store::*` so existing import sites compile
//! unchanged.
//!
//! The store-migration value types ([`DomainMigration`], [`MigrationTx`])
//! that previously lived in `nmp-core::substrate::domain` moved with the
//! store — they are consumed by `EventStore::run_migrations` and that's the
//! only seam they cross. `nmp-core::substrate` re-exports them so the
//! substrate surface (`nmp_core::substrate::{DomainMigration, MigrationTx}`)
//! is preserved.
//!
//! # Backends
//!
//! - `MemEventStore`: always compiled; used in tests and pre-M15 web builds.
//! - `LmdbEventStore`: compiled always but only functional with
//!   `--features lmdb-backend` (gates the heed / nostr-database / nmp-nostr-lmdb
//!   dependency graph).
//!
//! See `docs/design/lmdb/trait.md` for the full design specification.

mod domain_migration;
mod events;
mod lmdb;
mod mem;
pub mod types;

pub use domain_migration::{DomainMigration, MigrationTx};
pub use events::{ClaimGuard, DomainHandle, DomainScanIter, EventIter, EventStore};
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
