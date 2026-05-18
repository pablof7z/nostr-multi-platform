//! Substrate-level store types shared by `nmp-core` and every protocol crate.
//!
//! What lives here:
//! - the pure data types under [`types`] (`StoredEvent`, `RawEvent`,
//!   `VerifiedEvent`, `StoreError`, `EventId`, `PubKey`, watermarks, GC budget,
//!   tombstones, query, etc.) — see `store/types/`.
//! - the [`DomainBackend`] trait — the storage seam that each backend
//!   (`MemEventStore`, `LmdbEventStore`, …) implements.
//! - [`DomainHandle`] — module-scoped namespace handle wrapping an
//!   `Arc<dyn DomainBackend>`. The per-NIP `DomainModule` impls write
//!   through this.
//!
//! What stays in `nmp-core::store`:
//! - the `EventStore` trait itself (it references the `DomainMigration` type
//!   from [`crate::substrate::domain`] plus the backend constructors).
//! - the in-memory and LMDB backend impls (including the concrete
//!   `DomainBackend` impls for each).
//!
//! # Trait seam (PD-029 option A)
//!
//! Before PD-029, `DomainHandleInner` was an enum with `Mem` / `Lmdb` variants
//! whose `Lmdb` variant carried `Arc<crate::store::lmdb::Inner>` — an
//! `nmp-core`-private type. That tied substrate-types to LMDB internals and
//! blocked the original 22657bf extract from landing on top of T136b.
//!
//! The trait seam dissolves the coupling: substrate-types declares the
//! `DomainBackend` trait, and `nmp-core` provides one impl per backend. Adding
//! a third backend (e.g. SQLite) means writing one more `impl DomainBackend`
//! in `nmp-core` — substrate-types is unchanged.
//!
//! Doctrine D6: every operation here returns `Result<_, StoreError>` — no
//! panics, no FFI exceptions.

pub mod types;

use std::sync::Arc;

pub use types::StoreError;

/// One materialized scan result — a `(key, value)` pair both owned as `Vec<u8>`.
/// Aliased to keep `DomainBackend::scan_prefix` readable (and below the
/// `clippy::type_complexity` cap).
pub type ScanEntry = (Vec<u8>, Vec<u8>);

// ─── DomainBackend trait ──────────────────────────────────────────────────────

/// Storage seam for a single domain namespace.
///
/// Implementations carry whatever state they need (a `HashMap` mutex for the
/// memory backend; an `Arc<lmdb::Inner>` + namespace for the LMDB backend).
/// The [`DomainHandle`] wrapper hands these methods through to per-NIP
/// `DomainModule` impls without exposing the backend-specific machinery.
///
/// All methods are sync; each call must be self-contained (no transactions
/// escape the method). Implementers must be `Send + Sync` so the kernel can
/// shuttle handles across thread boundaries.
///
/// PD-029 option A: this trait is what lets `nmp-substrate-types` stay
/// backend-agnostic. The trait method set is intentionally small — only
/// `put / get / delete / scan_prefix` need to differ between backends.
/// Secondary index scans (`scan_index`) live on `DomainHandle` and currently
/// delegate to `scan_prefix` for every backend.
pub trait DomainBackend: Send + Sync {
    /// Write a key/value pair into this namespace.
    fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError>;

    /// Read a value by key from this namespace. Returns `Ok(None)` if absent.
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    /// Delete a key. Returns `true` iff the key existed.
    fn delete(&self, key: &[u8]) -> Result<bool, StoreError>;

    /// Scan all `(key, value)` pairs whose key starts with `prefix`, in
    /// ascending key order. Materializes the snapshot — the implementation
    /// must not lend out a live cursor (both extant backends already
    /// materialize today).
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<ScanEntry>, StoreError>;
}

// ─── DomainHandle ─────────────────────────────────────────────────────────────

/// Type alias for domain scan iterators (boxed for object-safety + lifetime
/// erasure at the callsite).
pub type DomainScanIter<'a> =
    Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>;

/// A module-scoped handle into the domain store for one namespace.
///
/// The kernel does not give a `DraftsModule` handle to `SettingsModule` —
/// isolation is enforced at construction time in `domain_open()`. The
/// `namespace` field is retained purely for debug / introspection; the
/// wrapped backend is already namespace-scoped at construction time.
///
/// Design: `docs/design/lmdb/trait.md` §3, PD-029 option A.
pub struct DomainHandle {
    /// Namespace (e.g. `"nmp.reactions"`). Retained for debug / introspection
    /// only — the wrapped backend is already namespace-scoped.
    #[allow(dead_code)]
    pub namespace: &'static str,
    /// Backend-specific storage for this namespace.
    pub inner: Arc<dyn DomainBackend>,
}

impl DomainHandle {
    /// Construct a `DomainHandle` over a concrete `DomainBackend` impl.
    ///
    /// Backends call this from their `EventStore::domain_open` impl —
    /// `nmp-core`'s `MemEventStore::domain_open` constructs a
    /// `MemDomainBackend`, `LmdbEventStore::domain_open` constructs an
    /// `LmdbDomainBackend`, etc.
    pub fn new(namespace: &'static str, inner: Arc<dyn DomainBackend>) -> Self {
        Self { namespace, inner }
    }

    /// Write a key/value pair into this domain namespace.
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.inner.put(key, value)
    }

    /// Read a value by key from this domain namespace.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get(key)
    }

    /// Delete a key. Returns `true` if the key existed.
    pub fn delete(&self, key: &[u8]) -> Result<bool, StoreError> {
        self.inner.delete(key)
    }

    /// Scan all entries whose key starts with `prefix`.
    pub fn scan_prefix<'a>(&'a self, prefix: &[u8]) -> Result<DomainScanIter<'a>, StoreError> {
        let rows = self.inner.scan_prefix(prefix)?;
        Ok(Box::new(rows.into_iter().map(Ok)))
    }

    /// Scan entries via a named secondary index with the given key prefix.
    pub fn scan_index<'a>(
        &'a self,
        _index: &'static str,
        key_prefix: &[u8],
    ) -> Result<DomainScanIter<'a>, StoreError> {
        // No backend currently maintains a separate secondary index — both
        // store one flat map per namespace. Fall back to `scan_prefix`.
        self.scan_prefix(key_prefix)
    }
}
