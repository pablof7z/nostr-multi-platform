//! Event storage abstraction for NMP.
//!
//! # Crate placement
//!
//! Extracted from `nmp-core::store` (see `docs/architecture/crate-boundaries.md`
//! §9). `nmp-core` re-exports the surface as `pub(crate) mod store` — external
//! callers must depend on `nmp-store` directly (#1608/#1944, compat facade
//! narrowed to crate-internal).
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

mod domain_handle;
mod domain_migration;
mod events;
mod events_query_dispatch;
pub mod ingest_log;
pub(crate) mod interaction;
mod lmdb;
mod mem;
// OPFS-SQLite backend (#1007). wasm32-only (the engine's inherent methods are
// `#[cfg(target_arch = "wasm32")]`) and gated behind the `opfs-sqlite-backend`
// feature; native `--all-features` builds exclude it entirely.
#[cfg(all(target_arch = "wasm32", feature = "opfs-sqlite-backend"))]
mod opfs;
// D20 — wasm-safe time shim. All wasm-reachable code in this crate that
// needs `Instant` imports from here instead of directly from `std::time`.
pub(crate) mod time;
// #1811 — cache-side full-text search seam (noun-free vocabulary + shared
// tokenizer + CompiledIndexSpec). `nmp-core` compiles its protocol-aware
// SearchScopeProviders into the noun-free types here.
pub mod text_search;
pub mod types;

pub use domain_handle::{DomainHandle, DomainScanIter};
pub use domain_migration::{DomainMigration, MigrationTx};
pub use events::{EventIter, EventStore};
pub use ingest_log::{
    DeleteReason, LogOp, LogRetentionClaim, PullGap, PullPage, ScanLogResult, StoreLogEntry,
};
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
// #1007 — OPFS-SQLite `EventStore` backend (browser-durable). PR-7's
// `nmp-browser-runtime` constructs it via `OpfsSqliteEventStore::open(...).await`
// and injects it as `Arc<dyn EventStore>`.
#[cfg(all(target_arch = "wasm32", feature = "opfs-sqlite-backend"))]
pub use opfs::OpfsSqliteEventStore;
// #1811 — FTS public surface. Re-exported at the crate root so backends and
// `nmp-core`'s scope compiler import from `nmp_store::*`.
pub use text_search::{
    is_prefix_match, split_query_terms, tokenize, CompiledIndexSpec, ExtractFn, SearchDocumentKey,
    SearchField, SearchScopeId, SearchScore, TextSearchBudget, TextSearchHit, TextSearchOrder,
    TextSearchQuery, TextSearchStatus, MAX_TOKENS_PER_DOC, MIN_TOKEN_BYTES, TOKENIZER_VERSION,
};
pub use types::{
    coverage_key, coverage_key_parts, CoverageGuard, CoverageMatchFn, CoverageRow, DeleteFilter,
    DumpFormat, DumpStats, EventId, GcBudget, GcReport, InsertOutcome, ProvenanceEntry, PubKey,
    RawEvent, RejectReason, RelayUrl, StoreQuery, StoredEvent, TargetInteractionCounts,
    TombstoneOrigin, TombstoneRow, VerifiedEvent, COVERAGE_KEY_SEP, DEFAULT_DURABLE_EVENT_CEILING,
    GC_MAX_DURATION_MS, GC_MAX_EVENTS_PER_STEP,
};

// Re-export error types from types (defined there to avoid circular imports).
pub use types::{StoreError, VerifyError};

#[cfg(feature = "test-support")]
pub use domain_handle::failing_put_domain_handle_for_test;

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
pub use nmp_nostr_lmdb::ReplaceableKey;

// F-TTL — re-export the canonical kind predicates.
pub use nmp_kinds::{is_addressable, is_replaceable};

// F-TTL — stub freshness types for non-lmdb builds (tests, wasm).
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

    /// Stub cache type.
    pub type ReplaceableCache = HashMap<ReplaceableKey, u64>;
}

#[cfg(not(feature = "lmdb-backend"))]
pub use replaceable_stubs::{ReplaceableCache, ReplaceableKey};

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

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
