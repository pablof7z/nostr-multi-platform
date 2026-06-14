//! K3 Stage D1 — coverage-ledger read/write for the LMDB backend (ADR-0056 §3).
//!
//! Split out of `store_impl.rs` so the heed-txn logic lives in one place and the
//! trait impl stays a thin delegation. The `nmp-coverage` sub-db maps
//! `filter_hash || 0x1F || relay_url` → `covered_through` (8-byte BE
//! unix-seconds). See [`crate::CoverageRow`] for the downward-closed,
//! honest-coverage semantics.

use std::sync::Arc;

use super::Inner;
use crate::types::coverage_key;
use crate::StoreError;

/// Advance the coverage watermark for `(filter_hash, relay)` to
/// `max(existing, covered_through)` and commit.
///
/// Monotonic by construction (read-modify-write inside a single write txn): a
/// later completion can only raise the proven downward-closed bound.
pub(super) fn record_coverage(
    inner: &Arc<Inner>,
    filter_hash: &str,
    relay: &str,
    covered_through: u64,
) -> Result<(), StoreError> {
    let key = coverage_key(filter_hash, relay);
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("coverage write_txn: {e}")))?;
    let existing = inner
        .coverage
        .get(&txn, key.as_slice())
        .map_err(|e| StoreError::Io(format!("coverage get: {e}")))?
        .and_then(|v| v.get(..8).map(|b| u64::from_be_bytes(b.try_into().unwrap())))
        .unwrap_or(0);
    let next = existing.max(covered_through);
    // Only write when the bound actually advances — avoids a redundant page
    // dirty on a repeated EOSE/NEG-DONE for an already-covered window. A
    // `covered_through == 0` call (no coverage) is therefore a no-op, which is
    // correct: an empty ledger and a `covered_through = 0` row are equivalent.
    if next > existing {
        inner
            .coverage
            .put(&mut txn, key.as_slice(), &next.to_be_bytes())
            .map_err(|e| StoreError::Io(format!("coverage put: {e}")))?;
    }
    txn.commit()
        .map_err(|e| StoreError::Io(format!("coverage commit: {e}")))?;
    Ok(())
}

/// Read the coverage watermark for `(filter_hash, relay)`, or `None` if absent.
pub(super) fn get_coverage(
    inner: &Arc<Inner>,
    filter_hash: &str,
    relay: &str,
) -> Result<Option<u64>, StoreError> {
    let key = coverage_key(filter_hash, relay);
    let txn = inner
        .env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("coverage read_txn: {e}")))?;
    let value = inner
        .coverage
        .get(&txn, key.as_slice())
        .map_err(|e| StoreError::Io(format!("coverage get: {e}")))?
        .and_then(|v| v.get(..8).map(|b| u64::from_be_bytes(b.try_into().unwrap())));
    Ok(value)
}
