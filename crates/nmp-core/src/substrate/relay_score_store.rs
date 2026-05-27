//! Substrate trait for relay-author-score persistence (W2).
//!
//! # Crate-graph constraint
//! `nmp-store` does NOT depend on `nmp-core`. The store layer therefore uses
//! raw primitives `([u8;32], String, u32, u32, u64)` for cell tuples. The
//! kernel-side adapter (`kernel/relay_score_flush.rs`) converts
//! `RelayAuthorScoreMap::snapshot()` rows into this raw form before calling
//! `put_batch`.
//!
//! # D-constraints
//! - D0: substrate traits carry no protocol nouns.
//! - D4: the kernel (actor) is the sole writer — the trait's `put_batch` is
//!   expected to be called only from `Kernel::flush_relay_scores_if_dirty`.
//! - D6: methods return `Result<_, Box<dyn Error>>` (never panic, never `unwrap`).

/// Cell tuple used by both load and flush:
/// `(pubkey_bytes_32, canonical_relay_url, successes, failures, last_used_unix_s)`.
pub type ScoreCell = ([u8; 32], String, u32, u32, u64);

/// Persistence seam for relay-author score cells.
///
/// Production impl: `nmp-store`'s LMDB `relay-author-scores-v1` sub-db.
/// Test/default impl: [`NoopRelayAuthorScoreStore`].
///
/// Intentionally NOT `Send` in the DI contract because D4 grants the kernel
/// actor exclusive ownership; use `Box<dyn RelayAuthorScoreStore>` (not Arc).
pub trait RelayAuthorScoreStore {
    /// Bulk-load all persisted cells at kernel startup. Called once from
    /// `Kernel::set_relay_score_store` immediately after injection. Unknown
    /// (un-persisted) pubkey/relay pairs return an empty `Vec`.
    fn load_all(&self) -> Result<Vec<ScoreCell>, Box<dyn std::error::Error>>;

    /// Persist a batch of score cells. Replaces (upserts) any existing cell
    /// for each `(pubkey, canonical_url)` key pair. Called by
    /// `Kernel::flush_relay_scores_if_dirty` on actor idle when the map is
    /// dirty. The batch may contain zero rows (noop). Implementations should
    /// be all-or-nothing within one transaction where possible (D6).
    ///
    /// `&mut self` encodes write-side exclusivity: only the kernel actor may
    /// call this, and only via `flush_relay_scores_if_dirty`. No shared-ref
    /// writes are permitted. D4.
    fn put_batch(&mut self, cells: Vec<ScoreCell>) -> Result<(), Box<dyn std::error::Error>>;
}

/// No-op implementation — never persists anything. Default when no LMDB
/// store has been injected (in-memory-only kernel mode, CI tests without
/// `--features lmdb-backend`).
pub struct NoopRelayAuthorScoreStore;

impl RelayAuthorScoreStore for NoopRelayAuthorScoreStore {
    fn load_all(&self) -> Result<Vec<ScoreCell>, Box<dyn std::error::Error>> {
        Ok(Vec::new())
    }

    fn put_batch(&mut self, _cells: Vec<ScoreCell>) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

#[cfg(feature = "lmdb-backend")]
pub struct LmdbRelayAuthorScoreStore {
    backend: LmdbRelayAuthorScoreBackend,
}

#[cfg(feature = "lmdb-backend")]
enum LmdbRelayAuthorScoreBackend {
    Store(crate::store::LmdbEventStore),
    Path(std::path::PathBuf),
}

#[cfg(feature = "lmdb-backend")]
impl LmdbRelayAuthorScoreStore {
    #[must_use]
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            backend: LmdbRelayAuthorScoreBackend::Path(path.into()),
        }
    }

    #[must_use]
    pub fn from_event_store(store: crate::store::LmdbEventStore) -> Self {
        Self {
            backend: LmdbRelayAuthorScoreBackend::Store(store),
        }
    }

    fn with_store<T>(
        &self,
        f: impl FnOnce(&crate::store::LmdbEventStore) -> Result<T, crate::store::StoreError>,
    ) -> Result<T, crate::store::StoreError> {
        match &self.backend {
            LmdbRelayAuthorScoreBackend::Store(store) => f(store),
            LmdbRelayAuthorScoreBackend::Path(path) => {
                let store = crate::store::LmdbEventStore::open(path)?;
                f(&store)
            }
        }
    }
}

#[cfg(feature = "lmdb-backend")]
impl RelayAuthorScoreStore for LmdbRelayAuthorScoreStore {
    fn load_all(&self) -> Result<Vec<ScoreCell>, Box<dyn std::error::Error>> {
        Ok(self.with_store(crate::store::relay_scores::load_all_raw)?)
    }

    fn put_batch(&mut self, cells: Vec<ScoreCell>) -> Result<(), Box<dyn std::error::Error>> {
        Ok(self.with_store(|store| crate::store::relay_scores::put_batch_raw(store, cells))?)
    }
}
