//! Phase 3 + 3b of `gc_step`: tombstone and address-tombstone purge.
//!
//! Extracted from `gc.rs` to keep that file under the 500-LOC hard cap
//! (AGENTS.md). Entry point is `purge_tombstones` — called exclusively by
//! `gc_step` inside the same `gc.rs` after Phase 1 (NIP-40) and Phase 2 (LRU).
//!
//! Both phases run inside their own `write_txn`s (separate from Phase 1/2
//! txns), gated by the V-117 heuristic (`GC_TOMBSTONE_PURGE_INTERVAL_SECS`)
//! to avoid O(tombstones) work on every pass.

#![cfg(feature = "lmdb-backend")]

use std::sync::Arc;

use super::{tombstones, Inner};
use crate::types::GcReport;
use crate::StoreError;

/// Mirrored from `mem/mod.rs:75`.
const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 3600;

/// Purge stale id-tombstones (Phase 3) and address-tombstones (Phase 3b).
///
/// Gating and `gc_last_tombstone_purge_secs` update are handled by the caller
/// (`gc_step`) so this function always runs (caller already checked the gate).
pub(super) fn purge_tombstones(
    inner: &Arc<Inner>,
    now_secs: u64,
    report: &mut GcReport,
) -> Result<(), StoreError> {
    // ── Phase 3: Purge old id-tombstones ─────────────────────────────────────
    {
        let mut txn = inner
            .env
            .write_txn()
            .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
        let mut stale_keys: Vec<Vec<u8>> = Vec::new();
        for entry in inner
            .tombstones
            .iter(&txn)
            .map_err(|e| StoreError::Io(format!("tomb iter: {e}")))?
        {
            let (k, v) = entry.map_err(|e| StoreError::Io(format!("tomb step: {e}")))?;
            let row = tombstones::decode_row(v)?;
            if now_secs.saturating_sub(row.deleted_at) > TOMBSTONE_MAX_AGE_SECS {
                stale_keys.push(k.to_vec());
            }
        }
        report.tombstones_purged = stale_keys.len();
        for k in stale_keys {
            inner
                .tombstones
                .delete(&mut txn, &k)
                .map_err(|e| StoreError::Io(format!("tomb del: {e}")))?;
        }
        txn.commit()
            .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    }

    // ── Phase 3b: Purge old address tombstones ────────────────────────────────
    //
    // addr_tombstones guard param-replaceable re-inserts when an event arrives
    // after the kind:5 `a`-tag delete that covered its coordinate.  The gate is
    // `tomb.deleted_at >= event.created_at` — so any new version with a HIGHER
    // created_at bypasses the gate regardless of whether the tombstone is present.
    // A purged addr tombstone therefore only allows stale copies (created_at <=
    // the original delete timestamp) to re-enter, which is identical to the
    // class of stale re-deliveries the per-id tombstone policy already accepts
    // after 90 days.  Safety: same retention argument as id-tombstones.
    {
        let mut txn = inner
            .env
            .write_txn()
            .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
        let mut stale_addr_keys: Vec<Vec<u8>> = Vec::new();
        for entry in inner
            .addr_tombstones
            .iter(&txn)
            .map_err(|e| StoreError::Io(format!("addr-tomb iter: {e}")))?
        {
            let (k, v) = entry.map_err(|e| StoreError::Io(format!("addr-tomb step: {e}")))?;
            let row = tombstones::decode_row(v)?;
            if now_secs.saturating_sub(row.deleted_at) > TOMBSTONE_MAX_AGE_SECS {
                stale_addr_keys.push(k.to_vec());
            }
        }
        report.addr_tombstones_purged = stale_addr_keys.len();
        for k in stale_addr_keys {
            inner
                .addr_tombstones
                .delete(&mut txn, &k)
                .map_err(|e| StoreError::Io(format!("addr-tomb del: {e}")))?;
        }
        txn.commit()
            .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    }

    Ok(())
}
