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
// D20 — wasm-safe time shim. All wasm-reachable code in this crate that
// needs `Instant` imports from here instead of directly from `std::time`.
pub(crate) mod time;
pub mod types;

pub use domain_migration::{DomainMigration, MigrationTx};
pub use events::{DomainHandle, DomainScanIter, EventIter, EventStore};
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
    coverage_key, coverage_key_parts, CoverageGuard, CoverageMatchFn, CoverageRow, DeleteFilter,
    DumpFormat, DumpStats, EventId, GcBudget, GcReport, InsertOutcome, ProvenanceEntry, PubKey,
    RawEvent, RejectReason, RelayUrl, StoreQuery, StoredEvent, TombstoneOrigin, TombstoneRow,
    VerifiedEvent, COVERAGE_KEY_SEP, DEFAULT_DURABLE_EVENT_CEILING, GC_MAX_DURATION_MS,
    GC_MAX_EVENTS_PER_STEP, HOT_EVENT_CEILING,
};

// Re-export error types from types (defined there to avoid circular imports).
pub use types::{StoreError, VerifyError};

// ── Test-support: LMDB materialization counter ────────────────────────────────
//
// Re-exported so `nmp-testing` integration tests can reach
// `nmp_store::reset_conversion_count()` / `nmp_store::conversion_count()`
// without spelling out the internal `lmdb::query_streaming` path.
// Only compiled when lmdb-backend is on (the counter lives in LMDB code).
#[cfg(all(feature = "lmdb-backend", any(test, feature = "test-support")))]
pub use lmdb::{conversion_count, reset_conversion_count};

// F-TTL — re-export replaceable freshness types from nmp-nostr-lmdb.
// Only available when lmdb-backend is enabled (the module owns the LMDB types).
#[cfg(feature = "lmdb-backend")]
pub use nmp_nostr_lmdb::{is_parameterized_replaceable, is_replaceable, ReplaceableKey};

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

    /// Check if a kind is parameterized replaceable / addressable (NIP-01).
    ///
    /// Delegates to [`nostr::Kind::is_addressable`] — the single source of
    /// truth — so the non-LMDB build classifies kinds identically to the
    /// LMDB build. Addressable is the `30000..=39999` range only.
    pub fn is_parameterized_replaceable(kind: u32) -> bool {
        nostr::Kind::from(kind as u16).is_addressable()
    }

    /// Check if a kind is regular replaceable (NIP-01).
    ///
    /// Delegates to [`nostr::Kind::is_replaceable`]. Regular replaceable is
    /// kinds `0`, `3`, `41` and the `10000..=19999` range — NOT the broad
    /// `kind < 20000` the prior hand-rolled stub used.
    pub fn is_replaceable(kind: u32) -> bool {
        nostr::Kind::from(kind as u16).is_replaceable()
    }

    /// Stub cache type.
    pub type ReplaceableCache = HashMap<ReplaceableKey, u64>;
}

#[cfg(not(feature = "lmdb-backend"))]
pub use replaceable_stubs::{
    is_parameterized_replaceable, is_replaceable, ReplaceableCache, ReplaceableKey,
};

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

// ── Cross-crate internal seam for nmp-core ────────────────────────────────
//
// `nmp-core` is the only legitimate cross-crate consumer of
// `VerifiedEvent::from_store_verified_unchecked`. The method is `pub(crate)`
// in `nmp-store`; this hidden module re-exports a thin free-function wrapper
// so `nmp-core` can reach it through a deliberate, grep-visible path
// (`nmp_store::__nmp_core_internal::from_store_verified_unchecked`) without
// promoting the constructor to the general public API surface.
//
// Convention mirrors `nmp_core::__ffi_internal`: the `__` prefix is the
// signal that this module is an extraction seam, not a public API.
// App crates and protocol crates MUST NOT import from here.
#[doc(hidden)]
pub mod __nmp_core_internal {
    use crate::types::{RawEvent, VerifiedEvent};

    /// Re-export of [`VerifiedEvent::from_store_verified_unchecked`] for
    /// `nmp-core`'s cache-serve path.
    ///
    /// See the safety contract on the private method: the caller MUST
    /// guarantee that `raw` was already verified at a prior trust boundary.
    #[must_use]
    #[inline]
    pub fn from_store_verified_unchecked(raw: RawEvent) -> VerifiedEvent {
        VerifiedEvent::from_store_verified_unchecked(raw)
    }
}
