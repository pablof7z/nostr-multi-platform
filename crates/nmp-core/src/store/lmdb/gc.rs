//! GC step for the LMDB backend.
//!
//! Mirrors `mem/gc.rs::gc_step`:
//!   * Reap NIP-40 expired events (up to budget.max_events_per_step).
//!   * Purge tombstones older than `TOMBSTONE_MAX_AGE_SECS`.
//!   * Honors `budget.max_duration_ms` between phases.
//!
//! LRU eviction is not implemented in this milestone — Mem doesn't have one
//! either; `gc_step` reports `lru_evicted = 0`. Future work tracked under
//! M4 GC tuning.

use std::sync::Arc;

use nostr::prelude::*;

use super::{provenance, tombstones, Inner};
use crate::store::types::{EventId, GcBudget, GcReport, TombstoneOrigin, TombstoneRow};
use crate::store::StoreError;

/// Mirrored from `mem/mod.rs:45`.
const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 3600;

pub(super) fn gc_step(
    inner: &Arc<Inner>,
    budget: GcBudget,
) -> Result<GcReport, StoreError> {
    let start = std::time::Instant::now();
    let mut report = GcReport::default();

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // ── Reap expired events ───────────────────────────────────────────────
    let expired: Vec<(EventId, u64)> = {
        let mut out = Vec::new();
        let txn = inner
            .lmdb
            .read_txn()
            .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
        let iter = inner
            .lmdb
            .query(&txn, Filter::new())
            .map_err(|e| StoreError::Io(format!("query: {e}")))?;
        for ev in iter {
            // Decode just enough to find the expiration tag.
            let owned: nostr::Event = ev.into_owned();
            if let Some(exp_tag) = owned.tags.iter().find(|t| {
                t.as_slice().first().map(|s| s == "expiration").unwrap_or(false)
            }) {
                if let Some(val) = exp_tag.as_slice().get(1) {
                    if let Ok(exp) = val.parse::<u64>() {
                        if exp <= now_secs {
                            let mut id = [0u8; 32];
                            id.copy_from_slice(owned.id.as_bytes());
                            out.push((id, exp));
                        }
                    }
                }
            }
            if out.len() >= budget.max_events_per_step {
                break;
            }
        }
        out
    };

    {
        let mut txn = inner
            .env
            .write_txn()
            .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
        for (id, _exp) in &expired {
            let f = Filter::new().id(
                nostr::EventId::from_slice(id)
                    .map_err(|e| StoreError::Encoding(format!("id: {e}")))?,
            );
            inner
                .lmdb
                .delete(&mut txn, f)
                .map_err(|e| StoreError::Io(format!("del: {e}")))?;
            provenance::delete(inner.provenance, &mut txn, id)?;
            tombstones::put(
                inner.tombstones,
                &mut txn,
                id,
                &TombstoneRow {
                    target_id: *id,
                    kind5_event_id: None,
                    deleter_pubkey: None,
                    deleted_at: now_secs,
                    sources: vec![],
                    origin: TombstoneOrigin::NIP40Expiry,
                },
            )?;
            report.expired_reaped += 1;
            if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
                break;
            }
        }
        txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    }

    // ── Purge old tombstones ──────────────────────────────────────────────
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
            let row = decode_row(v)?;
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
        txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    }

    report.duration_ms = start.elapsed().as_millis() as u32;
    Ok(report)
}

#[derive(serde::Deserialize)]
struct PersistRow {
    target_id: [u8; 32],
    kind5_event_id: Option<[u8; 32]>,
    deleter_pubkey: Option<[u8; 32]>,
    deleted_at: u64,
    sources: Vec<String>,
    origin: TombstoneOrigin,
}

fn decode_row(bytes: &[u8]) -> Result<TombstoneRow, StoreError> {
    let p: PersistRow = serde_json::from_slice(bytes)
        .map_err(|e| StoreError::Encoding(format!("tomb decode: {e}")))?;
    Ok(TombstoneRow {
        target_id: p.target_id,
        kind5_event_id: p.kind5_event_id,
        deleter_pubkey: p.deleter_pubkey,
        deleted_at: p.deleted_at,
        sources: p.sources,
        origin: p.origin,
    })
}
