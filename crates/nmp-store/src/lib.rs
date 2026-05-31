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
// W2 — relay-author-score encode/decode helpers. Gated on `lmdb-backend`
// because the implementation is LMDB-specific. Callers that build
// `LmdbRelayAuthorScoreStore` consume `load_all_raw` / `put_batch_raw`
// through `nmp_core::store::relay_scores::*` (re-exported via nmp-core).
#[cfg(feature = "lmdb-backend")]
pub mod relay_scores {
    pub use super::lmdb::relay_scores::{load_all_raw, put_batch_raw};
}
pub use mem::MemEventStore;
pub use types::{
    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
    InsertOutcome, ProvenanceEntry, PubKey, RawEvent, RejectReason, RelayUrl, StoreQuery,
    StoredEvent, SyncMethod, TombstoneOrigin, TombstoneRow, VerifiedEvent, WatermarkKey,
    WatermarkRow,
};

// Re-export error types from types (defined there to avoid circular imports).
pub use types::{StoreError, VerifyError};

// F-TTL — re-export replaceable freshness types from nmp-nostr-lmdb.
// Only available when lmdb-backend is enabled (the module owns the LMDB types).
#[cfg(feature = "lmdb-backend")]
pub use nmp_nostr_lmdb::{
    is_parameterized_replaceable, is_replaceable, ReplaceableKey,
};

// F-TTL — stub implementations for non-lmdb builds (tests, wasm).
// These allow the code to compile but the kernel will never use them
// (reverify queue and freshness store operations are no-ops in MemEventStore).
#[cfg(not(feature = "lmdb-backend"))]
pub mod replaceable_stubs {
    use std::collections::HashMap;

    /// Stub ReplaceableKey for non-LMDB builds.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub enum ReplaceableKey {
        /// Regular replaceable event: identified by kind and author pubkey.
        Regular { kind: u32, pubkey: [u8; 32] },
        /// Parameterized replaceable event: identified by kind, author pubkey, and d-tag.
        Parameterized {
            kind: u32,
            pubkey: [u8; 32],
            d_tag: String,
        },
    }

    impl ReplaceableKey {
        /// Get the kind for this key.
        pub fn kind(&self) -> u32 {
            match self {
                Self::Regular { kind, .. } | Self::Parameterized { kind, .. } => *kind,
            }
        }
    }

    /// Check if a kind is parameterized replaceable (NIP-01).
    pub fn is_parameterized_replaceable(kind: u32) -> bool {
        (kind >= 20000 && kind < 30000) || (kind >= 30000 && kind < 40000)
    }

    /// Check if a kind is replaceable (NIP-01).
    pub fn is_replaceable(kind: u32) -> bool {
        kind < 20000 || (kind >= 30000 && kind < 40000)
    }

    /// Stub cache type.
    pub type ReplaceableCache = HashMap<ReplaceableKey, u64>;
}

#[cfg(not(feature = "lmdb-backend"))]
pub use replaceable_stubs::{is_parameterized_replaceable, is_replaceable, ReplaceableKey};

use std::path::PathBuf;

/// Storage backend selector.
#[derive(Clone, Debug)]
pub enum StorageBackend {
    Memory,
    Lmdb { path: PathBuf },
}

/// Factory: construct a `Box<dyn EventStore>` from a backend selector.
pub fn open_event_store(backend: &StorageBackend) -> Result<Box<dyn EventStore>, StoreError> {
    match backend {
        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
        StorageBackend::Lmdb { path } => Ok(Box::new(LmdbEventStore::open(path)?)),
    }
}
