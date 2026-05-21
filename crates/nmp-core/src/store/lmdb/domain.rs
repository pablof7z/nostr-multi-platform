//! Domain rows + migrations for the LMDB backend.
//!
//! Single `nmp-domain-data` sub-db (chosen over one sub-db per namespace to
//! avoid exhausting `max_dbs`): each key is `namespace_bytes || 0x00 || user_key`.
//! Namespace schema versions live in `nmp-domain-versions`.

use std::sync::Arc;

use super::Inner;
use crate::store::events::{DomainHandle, DomainHandleInner};
use crate::store::StoreError;
use crate::substrate::DomainMigration;

/// Compose `namespace || 0x00 || user_key` for storage in the shared sub-db.
fn full_key(namespace: &str, user_key: &[u8]) -> Vec<u8> {
    let mut k = Vec::with_capacity(namespace.len() + 1 + user_key.len());
    k.extend_from_slice(namespace.as_bytes());
    k.push(0u8);
    k.extend_from_slice(user_key);
    k
}

/// Just the prefix (`namespace || 0x00`) for prefix scans.
fn ns_prefix(namespace: &str) -> Vec<u8> {
    let mut p = Vec::with_capacity(namespace.len() + 1);
    p.extend_from_slice(namespace.as_bytes());
    p.push(0u8);
    p
}

pub(crate) fn put(
    inner: &Arc<Inner>,
    namespace: &str,
    key: &[u8],
    value: &[u8],
) -> Result<(), StoreError> {
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    let k = full_key(namespace, key);
    inner
        .domain_data
        .put(&mut txn, &k, value)
        .map_err(|e| StoreError::Io(format!("dom put: {e}")))?;
    txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))
}

pub(crate) fn get(
    inner: &Arc<Inner>,
    namespace: &str,
    key: &[u8],
) -> Result<Option<Vec<u8>>, StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let k = full_key(namespace, key);
    Ok(inner
        .domain_data
        .get(&txn, &k)
        .map_err(|e| StoreError::Io(format!("dom get: {e}")))?
        .map(|v| v.to_vec()))
}

pub(crate) fn delete(
    inner: &Arc<Inner>,
    namespace: &str,
    key: &[u8],
) -> Result<bool, StoreError> {
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    let k = full_key(namespace, key);
    let removed = inner
        .domain_data
        .delete(&mut txn, &k)
        .map_err(|e| StoreError::Io(format!("dom del: {e}")))?;
    txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    Ok(removed)
}

/// One materialized scan entry: `(key, value)` both owned `Vec<u8>`. Local
/// alias keeps `scan_prefix` below the `clippy::type_complexity` cap.
type ScanEntry = (Vec<u8>, Vec<u8>);

pub(crate) fn scan_prefix(
    inner: &Arc<Inner>,
    namespace: &str,
    user_prefix: &[u8],
) -> Result<Vec<ScanEntry>, StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let mut full_prefix = ns_prefix(namespace);
    let prefix_len = full_prefix.len();
    full_prefix.extend_from_slice(user_prefix);
    let mut out = Vec::new();
    for entry in inner
        .domain_data
        .prefix_iter(&txn, &full_prefix)
        .map_err(|e| StoreError::Io(format!("dom iter: {e}")))?
    {
        let (k, v) = entry.map_err(|e| StoreError::Io(format!("dom step: {e}")))?;
        if k.len() < prefix_len {
            continue;
        }
        let user_k = k[prefix_len..].to_vec();
        out.push((user_k, v.to_vec()));
    }
    Ok(out)
}

pub(super) fn domain_open(
    inner: &Arc<Inner>,
    namespace: &'static str,
) -> Result<DomainHandle, StoreError> {
    Ok(DomainHandle {
        inner: DomainHandleInner::Lmdb {
            namespace,
            backend: Arc::clone(inner),
        },
    })
}

pub(super) fn run_migrations(
    inner: &Arc<Inner>,
    namespace: &'static str,
    target_version: u32,
    migrations: &[DomainMigration],
) -> Result<(), StoreError> {
    // Read current version.
    let current = {
        let txn = inner
            .lmdb
            .read_txn()
            .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
        match inner
            .domain_versions
            .get(&txn, namespace.as_bytes())
            .map_err(|e| StoreError::Io(format!("ver get: {e}")))?
        {
            Some(v) if v.len() == 4 => u32::from_be_bytes([v[0], v[1], v[2], v[3]]),
            _ => 0,
        }
    };

    if current > target_version {
        return Err(StoreError::SchemaTooNew {
            namespace: namespace.to_string(),
            on_disk: current,
            expected: target_version,
        });
    }
    if current == target_version {
        return Ok(());
    }

    // Apply migrations in order; each migration's writes are recorded in a
    // `MigrationTx` then applied as a batch.
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    for m in migrations {
        if m.from_version < current || m.from_version >= target_version {
            continue;
        }
        let mut tx = crate::substrate::MigrationTx::default();
        (m.apply)(&mut tx).map_err(|reason| StoreError::MigrationFailed { // doctrine-allow: D15 — `Migration::apply` is an internal substrate-registered migration closure run at store init, not a host FFI seam; loud panic here is the intended bug-surfacing behaviour (mirrors the actor command drain allowlist rationale)
            namespace: namespace.to_string(),
            from: m.from_version,
            to: m.to_version,
            reason,
        })?;
        for (k, v) in tx.writes() {
            let full = full_key(namespace, k);
            inner
                .domain_data
                .put(&mut txn, &full, v)
                .map_err(|e| StoreError::Io(format!("mig put: {e}")))?;
        }
    }

    inner
        .domain_versions
        .put(&mut txn, namespace.as_bytes(), &target_version.to_be_bytes())
        .map_err(|e| StoreError::Io(format!("ver put: {e}")))?;
    txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))
}
