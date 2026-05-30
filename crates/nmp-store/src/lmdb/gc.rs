//! GC step for the LMDB backend.
//!
//! Mirrors `mem/gc.rs::gc_step`:
//!   * Reap NIP-40 expired events (up to budget.max_events_per_step).
//!   * LRU-evict un-pinned events when store size exceeds `budget.max_total_events`.
//!   * Purge tombstones older than `TOMBSTONE_MAX_AGE_SECS`.
//!   * Honors `budget.max_duration_ms` between phases.
//!
//! V-60 LRU design notes:
//!
//! Access tracking uses a persisted `nmp-lru-access` sub-db (event_id → seq u64
//! BE) plus an in-memory `AtomicU64` counter on `Inner::lru_seq`.  Using a
//! monotonic counter rather than wall-clock time avoids introducing a D7 surface
//! on the read path while still providing a strict total order (no ties).
//!
//! Trade-off accepted: stamping `get_by_id` hits converts one read-txn into one
//! write-txn per point-read.  This is bounded to point-reads only (bulk scans
//! do NOT stamp, limiting write-amplification).  The alternative — wall-clock in
//! a read-txn — would reintroduce a D7 violation.
//!
//! Eviction skips pinned events (the union of all `claims` sub-db entries).
//! No tombstone is written for LRU-evicted events: they remain valid Nostr
//! events and may be re-fetched from a relay.

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use nostr::prelude::*;

use super::{provenance, tombstones, Inner};
use crate::types::{EventId, GcBudget, GcReport, TombstoneOrigin, TombstoneRow};
use crate::StoreError;

/// Mirrored from `mem/mod.rs:75`.
const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 3600;

// ─── LRU stamp / delete helpers ──────────────────────────────────────────────

/// Record an LRU access for `id` in an existing write transaction.
///
/// Atomically increments `inner.lru_seq` and persists the new value to the
/// `lru_access` sub-db.  Called by `get_by_id` on a hit and by `insert` on
/// every new event so gc_step can order events by recency.
pub(super) fn lru_stamp(
    inner: &Arc<Inner>,
    txn: &mut heed::RwTxn,
    id: &EventId,
) -> Result<(), StoreError> {
    let seq = inner.lru_seq.fetch_add(1, Ordering::Relaxed) + 1;
    inner
        .lru_access
        .put(txn, id.as_slice(), &seq.to_be_bytes())
        .map_err(|e| StoreError::Io(format!("lru_stamp: {e}")))
}

/// Remove the LRU entry for `id` from an existing write transaction.
///
/// Called on every event deletion path (expiry, LRU eviction, kind:5, admin
/// purge) so the access index never contains dangling references.
pub(super) fn lru_delete(
    inner: &Arc<Inner>,
    txn: &mut heed::RwTxn,
    id: &EventId,
) -> Result<(), StoreError> {
    inner
        .lru_access
        .delete(txn, id.as_slice())
        .map_err(|e| StoreError::Io(format!("lru_delete: {e}")))?;
    Ok(())
}

// ─── gc_step ─────────────────────────────────────────────────────────────────

/// One bounded GC pass.
///
/// `now_secs` is the kernel clock as Unix seconds (D7 — caller-supplied, the
/// store never calls `SystemTime::now()` directly).
pub(super) fn gc_step(
    inner: &Arc<Inner>,
    budget: GcBudget,
    now_secs: u64,
) -> Result<GcReport, StoreError> {
    let start = std::time::Instant::now();
    let mut report = GcReport::default();

    // ── Phase 1: Reap NIP-40 expired events ──────────────────────────────
    let expired: Vec<EventId> = {
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
            let owned: nostr::Event = ev.into_owned();
            if let Some(exp_tag) = owned.tags.iter().find(|t| {
                t.as_slice()
                    .first()
                    .map(|s| s == "expiration")
                    .unwrap_or(false)
            }) {
                if let Some(val) = exp_tag.as_slice().get(1) {
                    if let Ok(exp) = val.parse::<u64>() {
                        if exp <= now_secs {
                            let mut id = [0u8; 32];
                            id.copy_from_slice(owned.id.as_bytes());
                            out.push(id);
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
        for id in &expired {
            let f = Filter::new().id(nostr::EventId::from_slice(id)
                .map_err(|e| StoreError::Encoding(format!("id: {e}")))?);
            inner
                .lmdb
                .delete(&mut txn, f)
                .map_err(|e| StoreError::Io(format!("del: {e}")))?;
            provenance::delete(inner.provenance, &mut txn, id)?;
            lru_delete(inner, &mut txn, id)?;
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
                txn.commit()
                    .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
                return finish(start, report);
            }
        }
        txn.commit()
            .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    }

    // ── Phase 2: LRU eviction ─────────────────────────────────────────────
    //
    // Only runs when a finite ceiling is configured (max_total_events < usize::MAX).
    // Pinned events (union of all `claims` sub-db keys) are never evicted.
    // No tombstone is written for LRU-evicted events.
    if budget.max_total_events < usize::MAX {
        let event_count: usize = {
            let txn = inner
                .lmdb
                .read_txn()
                .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
            let n = inner
                .lmdb
                .query(&txn, Filter::new())
                .map_err(|e| StoreError::Io(format!("count query: {e}")))?
                .count();
            drop(txn);
            n
        };

        if event_count > budget.max_total_events {
            // Collect pinned event ids from the claims sub-db.
            // Key layout per lmdb/claims.rs: claimer_u64(8 BE) || event_id(32) = 40 bytes.
            let pinned: BTreeSet<EventId> = {
                let txn = inner
                    .env
                    .read_txn()
                    .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
                let mut set = BTreeSet::new();
                for entry in inner
                    .claims
                    .iter(&txn)
                    .map_err(|e| StoreError::Io(format!("claims iter: {e}")))?
                {
                    let (k, _) =
                        entry.map_err(|e| StoreError::Io(format!("claims entry: {e}")))?;
                    if k.len() == 40 {
                        let mut id = [0u8; 32];
                        id.copy_from_slice(&k[8..40]);
                        set.insert(id);
                    }
                }
                set
            };

            // Read lru_access, filter out pinned, sort ascending by seq (oldest first).
            let mut candidates: Vec<(u64, EventId)> = {
                let txn = inner
                    .env
                    .read_txn()
                    .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
                let mut v = Vec::new();
                for entry in inner
                    .lru_access
                    .iter(&txn)
                    .map_err(|e| StoreError::Io(format!("lru iter: {e}")))?
                {
                    let (k, val) =
                        entry.map_err(|e| StoreError::Io(format!("lru entry: {e}")))?;
                    if k.len() == 32 && val.len() >= 8 {
                        let mut id = [0u8; 32];
                        id.copy_from_slice(k);
                        if !pinned.contains(&id) {
                            let seq = u64::from_be_bytes(val[..8].try_into().unwrap());
                            v.push((seq, id));
                        }
                    }
                }
                v.sort_unstable_by_key(|(seq, _)| *seq);
                v
            };

            let overage = event_count.saturating_sub(budget.max_total_events);
            let to_evict = overage.min(budget.max_events_per_step);
            candidates.truncate(to_evict);

            if !candidates.is_empty() {
                let mut txn = inner
                    .env
                    .write_txn()
                    .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
                for (_, id) in &candidates {
                    let f = Filter::new().id(
                        nostr::EventId::from_slice(id)
                            .map_err(|e| StoreError::Encoding(format!("id: {e}")))?,
                    );
                    inner
                        .lmdb
                        .delete(&mut txn, f)
                        .map_err(|e| StoreError::Io(format!("lru evict del: {e}")))?;
                    provenance::delete(inner.provenance, &mut txn, id)?;
                    lru_delete(inner, &mut txn, id)?;
                    report.lru_evicted += 1;
                    if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
                        txn.commit()
                            .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
                        return finish(start, report);
                    }
                }
                txn.commit()
                    .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
            }
        }
    }

    // ── Phase 3: Purge old tombstones ─────────────────────────────────────
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
        txn.commit()
            .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    }

    finish(start, report)
}

#[inline]
fn finish(start: std::time::Instant, mut report: GcReport) -> Result<GcReport, StoreError> {
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
