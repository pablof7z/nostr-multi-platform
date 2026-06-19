//! `DomainHandle` — a module-scoped key/value handle into the domain store.
//!
//! Split out of `events.rs` (which hosts the `EventStore` trait) to keep that
//! file under its size budget. See `docs/design/lmdb/trait.md` §3.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[cfg(feature = "lmdb-backend")]
use crate::lmdb::Inner;
use crate::StoreError;

/// Shared data map for a single domain namespace (memory backend).
type MemDomainData = Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>;

/// Backend-specific storage for a `DomainHandle`.
pub(crate) enum DomainHandleInner {
    Mem {
        #[allow(dead_code)] // Preserved for debug/domain isolation checks.
        namespace: &'static str,
        data: MemDomainData,
    },
    // LMDB variant — carries the namespace + a handle to the LMDB-side state.
    // The actual storage operations live in `crate::lmdb::domain`.
    #[cfg(feature = "lmdb-backend")]
    Lmdb {
        namespace: &'static str,
        backend: Arc<Inner>,
    },
}

/// Type alias for domain scan iterators.
pub type DomainScanIter<'a> = Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>;

/// A module-scoped handle into the domain store for one namespace.
///
/// The kernel does not give a `DraftsModule` handle to `SettingsModule` —
/// isolation is enforced at construction time in `domain_open()`.
///
/// Design: `docs/design/lmdb/trait.md` §3.
pub struct DomainHandle {
    pub(crate) inner: DomainHandleInner,
}

impl DomainHandle {
    /// Write a key/value pair into this domain namespace.
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        match &self.inner {
            DomainHandleInner::Mem { data, .. } => {
                data.lock()
                    .map_err(|e| StoreError::Io(e.to_string()))?
                    .insert(key.to_vec(), value.to_vec());
                Ok(())
            }
            #[cfg(feature = "lmdb-backend")]
            DomainHandleInner::Lmdb { namespace, backend } => {
                crate::lmdb::domain::put(backend, namespace, key, value)
            }
        }
    }

    /// Read a value by key from this domain namespace.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        match &self.inner {
            DomainHandleInner::Mem { data, .. } => Ok(data
                .lock()
                .map_err(|e| StoreError::Io(e.to_string()))?
                .get(key)
                .cloned()),
            #[cfg(feature = "lmdb-backend")]
            DomainHandleInner::Lmdb { namespace, backend } => {
                crate::lmdb::domain::get(backend, namespace, key)
            }
        }
    }

    /// Delete a key. Returns `true` if the key existed.
    pub fn delete(&self, key: &[u8]) -> Result<bool, StoreError> {
        match &self.inner {
            DomainHandleInner::Mem { data, .. } => Ok(data
                .lock()
                .map_err(|e| StoreError::Io(e.to_string()))?
                .remove(key)
                .is_some()),
            #[cfg(feature = "lmdb-backend")]
            DomainHandleInner::Lmdb { namespace, backend } => {
                crate::lmdb::domain::delete(backend, namespace, key)
            }
        }
    }

    /// Scan all entries whose key starts with `prefix`.
    pub fn scan_prefix<'a>(&'a self, prefix: &[u8]) -> Result<DomainScanIter<'a>, StoreError> {
        match &self.inner {
            DomainHandleInner::Mem { data, .. } => {
                let snapshot: Vec<(Vec<u8>, Vec<u8>)> = data
                    .lock()
                    .map_err(|e| StoreError::Io(e.to_string()))?
                    .iter()
                    .filter(|(k, _)| k.starts_with(prefix))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Ok(Box::new(snapshot.into_iter().map(Ok)))
            }
            #[cfg(feature = "lmdb-backend")]
            DomainHandleInner::Lmdb { namespace, backend } => {
                let rows = crate::lmdb::domain::scan_prefix(backend, namespace, prefix)?;
                Ok(Box::new(rows.into_iter().map(Ok)))
            }
        }
    }

    /// Scan entries via a named secondary index with the given key prefix.
    pub fn scan_index<'a>(
        &'a self,
        _index: &'static str,
        key_prefix: &[u8],
    ) -> Result<DomainScanIter<'a>, StoreError> {
        // For now both backends have a flat map per namespace — no separate
        // secondary indexes are maintained. Fall back to scan_prefix.
        self.scan_prefix(key_prefix)
    }
}
