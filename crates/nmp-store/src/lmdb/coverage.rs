//! K3 Stage D1 — coverage-ledger read/write for the LMDB backend (ADR-0056 §3).
//!
//! Split out of `store_impl.rs` so the heed-txn logic lives in one place and the
//! trait impl stays a thin delegation. The `nmp-coverage` sub-db maps
//! `filter_hash || 0x1F || relay_url` → `covered_through` (8-byte BE
//! unix-seconds). See [`crate::CoverageRow`] for the downward-closed,
//! honest-coverage semantics.

use std::sync::Arc;

use heed::RwTxn;

use super::Inner;
use crate::types::{coverage_key, CoverageGuard, COVERAGE_KEY_SEP};
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
        .and_then(|v| {
            v.get(..8)
                .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
        })
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
        .and_then(|v| {
            v.get(..8)
                .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
        });
    Ok(value)
}

/// K3 Stage D3 — the highest `covered_through` recorded for `filter_hash` across
/// all relays, or `None` if no relay has a row.
///
/// `filter_hash` is a fixed 16-hex-char prefix of every key for the shape
/// (`filter_hash || 0x1F || relay`), so we range-scan from the prefix and stop
/// at the first key that no longer starts with it. The ledger is small (one row
/// per active `(filter_hash, relay)`), so this read-txn scan is cheap.
pub(super) fn max_for_filter_hash(
    inner: &Arc<Inner>,
    filter_hash: &str,
) -> Result<Option<u64>, StoreError> {
    let mut prefix = filter_hash.as_bytes().to_vec();
    prefix.push(COVERAGE_KEY_SEP);
    let txn = inner
        .env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("coverage read_txn: {e}")))?;
    let mut best: Option<u64> = None;
    for entry in inner
        .coverage
        .prefix_iter(&txn, prefix.as_slice())
        .map_err(|e| StoreError::Io(format!("coverage prefix_iter: {e}")))?
    {
        let (_k, v) = entry.map_err(|e| StoreError::Io(format!("coverage entry: {e}")))?;
        if let Some(ts) = v
            .get(..8)
            .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
        {
            best = Some(best.map_or(ts, |m: u64| m.max(ts)));
        }
    }
    Ok(best)
}

/// K3 Stage D3 — every `(relay, covered_through)` row recorded for
/// `filter_hash`. Range-scans the `filter_hash || 0x1F` prefix and decodes the
/// relay half of each key. Used by the kernel to build the eviction backstop's
/// per-`(filter_hash, relay)` guard set.
pub(super) fn rows_for_filter_hash(
    inner: &Arc<Inner>,
    filter_hash: &str,
) -> Result<Vec<(String, u64)>, StoreError> {
    let mut prefix = filter_hash.as_bytes().to_vec();
    prefix.push(COVERAGE_KEY_SEP);
    let txn = inner
        .env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("coverage read_txn: {e}")))?;
    let mut out: Vec<(String, u64)> = Vec::new();
    for entry in inner
        .coverage
        .prefix_iter(&txn, prefix.as_slice())
        .map_err(|e| StoreError::Io(format!("coverage prefix_iter: {e}")))?
    {
        let (k, v) = entry.map_err(|e| StoreError::Io(format!("coverage entry: {e}")))?;
        if let (Some((_fh, relay)), Some(ts)) = (
            crate::types::coverage_key_parts(k),
            v.get(..8)
                .map(|b| u64::from_be_bytes(b.try_into().unwrap())),
        ) {
            out.push((relay, ts));
        }
    }
    Ok(out)
}

/// K3 Stage D3 backstop — lower the coverage row for `(filter_hash, relay)` to
/// `new_bound` **inside an existing write txn** (so the lowering commits
/// atomically with the GC deletes that made it necessary). Only writes when the
/// row exists and currently claims MORE than `new_bound` (downward-only). A
/// `new_bound` of `0` deletes the row rather than leave a misleading `0`-bound.
///
/// Unlike [`record_coverage`], this does NOT open its own txn — the caller (the
/// Phase-2 LRU eviction in `gc.rs`) passes the same `RwTxn` it deletes through,
/// which is the whole point: ledger coherence is only sound if the lower and the
/// delete are one transaction.
pub(super) fn lower_in_txn(
    inner: &Arc<Inner>,
    txn: &mut RwTxn<'_>,
    filter_hash: &str,
    relay: &str,
    new_bound: u64,
) -> Result<(), StoreError> {
    let key = coverage_key(filter_hash, relay);
    let existing = inner
        .coverage
        .get(txn, key.as_slice())
        .map_err(|e| StoreError::Io(format!("coverage get (lower): {e}")))?
        .and_then(|v| {
            v.get(..8)
                .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
        });
    let Some(existing) = existing else {
        return Ok(()); // no row to lower
    };
    if existing <= new_bound {
        return Ok(()); // already at/below the honest bound — never raise
    }
    if new_bound == 0 {
        inner
            .coverage
            .delete(txn, key.as_slice())
            .map_err(|e| StoreError::Io(format!("coverage delete (lower): {e}")))?;
    } else {
        inner
            .coverage
            .put(txn, key.as_slice(), &new_bound.to_be_bytes())
            .map_err(|e| StoreError::Io(format!("coverage put (lower): {e}")))?;
    }
    Ok(())
}

/// K3 Stage D3 backstop — lower each guard's coverage row to just below the
/// oldest evicted covered event for it, INSIDE the supplied Phase-2 write txn
/// (so the lowering commits atomically with the deletes). `new_bound =
/// oldest_evicted - 1` is the highest timestamp the ledger can still honestly
/// claim once the event AT that timestamp is gone; `0` clears the row (via
/// [`lower_in_txn`]). `min_evicted_covered[gi]` is `None` when guard `gi`
/// stranded no below-floor event this pass (row left untouched).
pub(super) fn lower_guards_in_txn(
    inner: &Arc<Inner>,
    txn: &mut RwTxn<'_>,
    guards: &[CoverageGuard],
    min_evicted_covered: &[Option<u64>],
) -> Result<(), StoreError> {
    for (gi, guard) in guards.iter().enumerate() {
        if let Some(oldest_evicted) = min_evicted_covered[gi] {
            let new_bound = oldest_evicted.saturating_sub(1);
            lower_in_txn(inner, txn, &guard.filter_hash, &guard.relay, new_bound)?;
        }
    }
    Ok(())
}
