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
//! Eviction skips pinned events.  The pin set is supplied by the caller
//! (`pins: &HashSet<EventId>`) — the kernel derives it on every GC pass from
//! `timeline`, `event_claims`, and the active open-interest registry (#1090
//! Stage 1).  No persisted claims sub-db exists any more; pins are ephemeral
//! per pass.  No tombstone is written for LRU-evicted events: they remain valid
//! Nostr events and may be re-fetched from a relay.
//!
//! V-118 Phase-1 expiration index (replaces the V-117 cursor):
//!
//! V-117 bounded the NIP-40 reaper with a `max_duration_ms` gate plus a cursor
//! keyed on `created_at`.  That cursor had a real defect (#1097): a block of
//! non-expired events sharing one `created_at` larger than one budget pass
//! parked the cursor forever, so Phase 1 never reached older expired events.
//!
//! V-118 removes the cursor.  A dedicated `nmp-expiry-index` sub-db keyed
//! `expiry_ts(8 BE) || event_id(32)` is maintained by insert + every delete
//! path (backfilled once on open for pre-V-118 stores — see
//! `open.rs::backfill_expiry_index`).  Phase 1 is now an O(expired) range scan
//! over that index for `expiry_ts ≤ now_secs`; non-expired events are invisible
//! to it, so a large non-expired block can never stall progress.  The budgets
//! still apply, and lowest-expiry-first key order means the next pass continues
//! from where the previous stopped — no cursor state to persist.

use std::collections::HashSet;
use std::ops::Bound;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use nostr::prelude::*;

use super::{provenance, tombstones, Inner};
use crate::types::{EventId, GcBudget, GcReport, TombstoneOrigin, TombstoneRow};
use crate::StoreError;

/// Mirrored from `mem/mod.rs:75`.
const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 3600;

/// Phase-3/3b tombstone scan runs at most once per hour (V-117).
///
/// Iterating every tombstone row inside a WRITE txn every 60 s causes
/// O(tombstones) work on the actor thread even when nothing is purgeable
/// (the 90-day age threshold means nothing is ripe in the first hour anyway).
/// Throttle to once per `GC_TOMBSTONE_PURGE_INTERVAL_SECS` using the
/// caller-supplied `now_secs` (D7).
pub const GC_TOMBSTONE_PURGE_INTERVAL_SECS: u64 = 3_600;

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

// ─── Expiry-index helpers (V-118, #1097) ─────────────────────────────────────

/// Key encoding for the expiry index: 8-byte BE u64 expiry_ts || 32-byte event_id.
///
/// Big-endian, NOT inverted, so lower timestamps sort first.  A range scan for
/// all entries with `expiry_ts ≤ now` is `(Unbounded, Excluded([now+1; 0..0]))`.
pub(super) fn expiry_index_key(expiry_ts: u64, id: &EventId) -> [u8; 40] {
    let mut k = [0u8; 40];
    k[..8].copy_from_slice(&expiry_ts.to_be_bytes());
    k[8..].copy_from_slice(id);
    k
}

/// Write `expiry_ts → event_id` into the expiry index within an existing write txn.
pub(super) fn expiry_index_put(
    inner: &Arc<Inner>,
    txn: &mut heed::RwTxn,
    expiry_ts: u64,
    id: &EventId,
) -> Result<(), StoreError> {
    let key = expiry_index_key(expiry_ts, id);
    inner
        .expiry_index
        .put(txn, &key, &[])
        .map_err(|e| StoreError::Io(format!("expiry_index put: {e}")))
}

/// Remove the expiry-index entry for `id` within an existing write txn using
/// the known `expiry_ts`.
///
/// O(1): constructs the exact 40-byte key and issues a single LMDB delete.
/// Returns `Ok(())` immediately if `expiry_ts` is `None` (the event carries no
/// expiration tag and therefore has no index entry).
///
/// Replaces the old `expiry_index_delete_id` which did an O(index) linear scan
/// because callers didn't always know the expiry timestamp.  All delete paths
/// now retrieve the expiry from the event before deleting it, enabling O(1)
/// cleanup.
pub(super) fn expiry_index_delete_exact(
    inner: &Arc<Inner>,
    txn: &mut heed::RwTxn,
    expiry_ts: Option<u64>,
    id: &EventId,
) -> Result<(), StoreError> {
    let Some(exp) = expiry_ts else {
        return Ok(()); // No expiration tag → no index entry to remove.
    };
    let key = expiry_index_key(exp, id);
    inner
        .expiry_index
        .delete(txn, &key)
        .map_err(|e| StoreError::Io(format!("expiry_index delete_exact: {e}")))?;
    Ok(())
}

// ─── gc_step ─────────────────────────────────────────────────────────────────

/// One bounded GC pass with an explicit derived pin set.
///
/// `now_secs` is the kernel clock as Unix seconds (D7 — caller-supplied, the
/// store never calls `SystemTime::now()` directly).
///
/// `pins` is the set of event ids to protect from Phase-2 LRU eviction.  The
/// kernel derives it on each call from `timeline`, `event_claims`, and the
/// active open-interest registry (#1090 Stage 1).  The store holds no persisted
/// claim state any more.
pub(super) fn gc_step(
    inner: &Arc<Inner>,
    budget: GcBudget,
    now_secs: u64,
    pins: &HashSet<EventId>,
) -> Result<GcReport, StoreError> {
    let start = std::time::Instant::now();
    let mut report = GcReport::default();

    // ── Phase 1: Reap NIP-40 expired events (V-118 expiry index, #1097) ──────
    //
    // O(expired) range scan over `nmp-expiry-index` for keys with
    // `expiry_ts ≤ now_secs` (encoding: expiry_ts(8 BE) || event_id(32)).  The
    // scan runs from the index start up to — but not including — the key
    // `[now_secs + 1; 0..0]`.  See the module-level doc for the full rationale.
    {
        // Upper bound: first key after every entry expired by `now_secs`
        // (saturating add so `now_secs == u64::MAX` degenerates to "scan all").
        let upper: [u8; 40] = {
            let mut k = [0u8; 40];
            k[..8].copy_from_slice(&now_secs.saturating_add(1).to_be_bytes());
            k
        };
        // Collect expired (index_key, event_id) up to budget in a read txn.
        let to_reap: Vec<([u8; 40], EventId)> = {
            let txn = inner
                .env
                .read_txn()
                .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
            let range = (Bound::Unbounded, Bound::Excluded(upper.as_slice()));
            let mut out: Vec<([u8; 40], EventId)> = Vec::new();
            for entry in inner
                .expiry_index
                .range(&txn, &range)
                .map_err(|e| StoreError::Io(format!("expiry_index range: {e}")))?
            {
                if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
                    break;
                }
                let (k, _) =
                    entry.map_err(|e| StoreError::Io(format!("expiry_index entry: {e}")))?;
                if k.len() != 40 {
                    continue;
                }
                let mut key_arr = [0u8; 40];
                key_arr.copy_from_slice(k);
                let mut id = [0u8; 32];
                id.copy_from_slice(&k[8..]);
                out.push((key_arr, id));
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
            for (index_key, id) in &to_reap {
                // Remove the index entry first (we already hold its exact key).
                inner
                    .expiry_index
                    .delete(&mut txn, index_key.as_slice())
                    .map_err(|e| StoreError::Io(format!("expiry del: {e}")))?;
                // Delete the event from the main store + NMP secondaries.
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
    }

    // ── Phase 2: LRU eviction ─────────────────────────────────────────────
    //
    // Only runs when a finite ceiling is configured (max_total_events < usize::MAX).
    // Pinned events (the caller-supplied `pins` set) are never evicted.
    // No tombstone is written for LRU-evicted events.
    //
    // V-117 fix: replace the O(N) `query(Filter::new()).count()` with an O(1)
    // LMDB stat call via `Lmdb::count`.  The nmp-nostr-lmdb fork's `count`
    // method uses `ci_index.len(txn)` for an empty filter — one MDB_stat
    // syscall instead of a full table scan.
    if budget.max_total_events < usize::MAX {
        let event_count: usize = {
            let txn = inner
                .lmdb
                .read_txn()
                .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
            // O(1): uses ci_index.len(txn) (LMDB MDB_stat) for the empty filter.
            inner
                .lmdb
                .count(&txn, Filter::new())
                .map_err(|e| StoreError::Io(format!("count: {e}")))?
        };

        if event_count > budget.max_total_events {
            // Pinned set is the caller-derived `pins` (#1090 Stage 1) — no
            // persisted claims sub-db is consulted any more.
            let pinned = pins;

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
    //
    // V-117 fix: gate Phase 3 and 3b behind a wall-clock heuristic.
    //
    // Iterating every tombstone row inside a WRITE txn and serde-decoding
    // each one is O(tombstones) work.  Since the retention window is 90 days,
    // nothing is ever purgeable in the first hour after a store is opened —
    // running the scan every 60 s wastes CPU even when it finds nothing.
    //
    // Gate: run at most once per GC_TOMBSTONE_PURGE_INTERVAL_SECS using the
    // caller-injected `now_secs` (D7-safe).  The `gc_last_tombstone_purge_secs`
    // field on `Inner` tracks when the scan last ran.
    let last_purge = inner
        .gc_last_tombstone_purge_secs
        .load(Ordering::Relaxed);
    let purge_due =
        now_secs.saturating_sub(last_purge) >= GC_TOMBSTONE_PURGE_INTERVAL_SECS;

    if purge_due {
        // Mark the purge as starting now so that even if we bail early (budget)
        // we do not re-enter the expensive scan in the same hour.
        inner
            .gc_last_tombstone_purge_secs
            .store(now_secs, Ordering::Relaxed);

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

        // ── Phase 3b: Purge old address tombstones ─────────────────────────
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
                let (k, v) =
                    entry.map_err(|e| StoreError::Io(format!("addr-tomb step: {e}")))?;
                let row = decode_row(v)?;
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
