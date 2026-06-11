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
//!
//! V-117 Phase-1 resumable cursor:
//!
//! Phase 1 (NIP-40 reaper) used to iterate the WHOLE store with no duration
//! check, causing O(store) actor stalls on large existing stores.  The fix
//! checks `max_duration_ms` inside the scan loop and persists a cursor
//! (`gc_phase1_cursor` on `Inner`) so each pass continues from the oldest
//! event it reached last time rather than restarting from the top.  After a
//! complete sweep the cursor resets to `None` so newly inserted (newer)
//! events are visited on the next pass.
//!
//! Cursor semantics: store the `created_at` timestamp (unix seconds) of the
//! last-scanned event as the `until` bound for the next pass.  Events sharing
//! that exact timestamp may be re-scanned on the next pass — this is correct
//! (re-checking a non-expired event is a no-op) and avoids complex per-id
//! tracking.

use std::collections::BTreeSet;
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
    //
    // V-117 fix: check max_duration_ms inside the scan loop, and persist a
    // resumable cursor so a large store is swept incrementally rather than
    // restarting from the top on every pass.
    //
    // Cursor discipline:
    //  - `cursor` = `Some(T)` means the previous pass stopped at an event
    //    with `created_at == T`.  We resume with `until(T)` which includes
    //    events at exactly T (correct: if they expired since the last pass
    //    they will be reaped; if they didn't, the check is a no-op).
    //  - `cursor` = `None` means start from the newest event.
    //  - After a full sweep (we reach the bottom of the store without hitting
    //    the budget), the cursor is reset to `None` so the next pass re-scans
    //    from the top, catching newly inserted events.
    //
    // KNOWN LIMITATION (V-118, GitHub issue #1097): because `until(T)` is an
    // inclusive bound, a block of NON-expired events sharing one `created_at`
    // that is larger than one budget pass parks the cursor at `T` forever —
    // every pass re-scans the same prefix and Phase 1 never reaches older
    // events.  Narrow (needs a bulk import with thousands of events on one
    // second) but real.  The durable fix is an `(expiry_ts → event_id)`
    // expiration index that removes this cursor entirely; see #1097.
    {
        let cursor_secs: Option<u64> = *inner
            .gc_phase1_cursor
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // Snapshot and unlock immediately — the scan is read-only and the
        // cursor update at the end of the block re-locks briefly.

        let filter = match cursor_secs {
            Some(t) => Filter::new().until(Timestamp::from_secs(t)),
            None => Filter::new(),
        };

        let expired: Vec<EventId> = {
            let mut out = Vec::new();
            let txn = inner
                .lmdb
                .read_txn()
                .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
            let iter = inner
                .lmdb
                .query(&txn, filter)
                .map_err(|e| StoreError::Io(format!("query: {e}")))?;

            let mut last_created_at: Option<u64> = None;
            let mut hit_budget_or_end = false;

            for ev in iter {
                // Duration gate: check budget BEFORE deserializing the next event.
                // This bounds actor-thread stall time even for huge stores.
                if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
                    hit_budget_or_end = true;
                    // Advance cursor to the last event's created_at so the next
                    // pass resumes just below it.  If we never saw an event
                    // (budget too tight, ~0 ms) keep the old cursor so we make
                    // forward progress on the next tick.
                    if let Some(t) = last_created_at {
                        *inner
                            .gc_phase1_cursor
                            .lock()
                            .unwrap_or_else(|p| p.into_inner()) = Some(t);
                    }
                    break;
                }

                let owned: nostr::Event = ev.into_owned();
                last_created_at = Some(owned.created_at.as_secs());

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
                    hit_budget_or_end = true;
                    // Event-count budget hit: persist cursor so we resume after
                    // the current position next pass.
                    if let Some(t) = last_created_at {
                        *inner
                            .gc_phase1_cursor
                            .lock()
                            .unwrap_or_else(|p| p.into_inner()) = Some(t);
                    }
                    break;
                }
            }

            // If we consumed the entire iterator without hitting a budget,
            // reset the cursor so the next pass re-starts from the top
            // (catching any newly inserted events).
            if !hit_budget_or_end {
                *inner
                    .gc_phase1_cursor
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = None;
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
    }

    // ── Phase 2: LRU eviction ─────────────────────────────────────────────
    //
    // Only runs when a finite ceiling is configured (max_total_events < usize::MAX).
    // Pinned events (union of all `claims` sub-db keys) are never evicted.
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
