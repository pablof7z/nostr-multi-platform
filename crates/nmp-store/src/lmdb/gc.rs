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

use super::{coverage, fts, gc_tombstones, ingest_log, provenance, tombstones, Inner};
use crate::ingest_log::DeleteReason;
use crate::types::{CoverageGuard, EventId, GcBudget, GcReport, TombstoneOrigin, TombstoneRow};
use crate::StoreError;

/// Phase-3/3b tombstone scan runs at most once per hour (V-117).
///
/// Iterating every tombstone row inside a WRITE txn every 60 s causes
/// O(tombstones) work on the actor thread even when nothing is purgeable
/// (the 90-day age threshold means nothing is ripe in the first hour anyway).
/// Throttle to once per `GC_TOMBSTONE_PURGE_INTERVAL_SECS` using the
/// caller-supplied `now_secs` (D7).
pub const GC_TOMBSTONE_PURGE_INTERVAL_SECS: u64 = 3_600;

// Secondary-index maintenance primitives live in `gc_index.rs` (LOC-cap
// split).  Re-export them so existing `gc::<name>` call sites in insert.rs,
// delete.rs, query.rs and below are unchanged.
pub(super) use super::gc_index::{
    expiry_index_delete_exact, expiry_index_put, freshness_key_from_event, lru_delete, lru_stamp,
};

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
    guards: &[CoverageGuard],
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
            // ADR-0058 §6 step-4: snapshot retention claims once for this reap txn.
            let retention_claims = inner.retention_claims_snapshot();
            for (index_key, id) in &to_reap {
                // Load the event before deletion to capture the freshness key
                // (Bug-2 fix: stale replaceable_freshness must be cleaned) and
                // its kind+tags (#1518: relay×kind index cleanup; #1519: IC decrement).
                let (freshness_key, kind, event_tags) = match inner
                    .lmdb
                    .get_event_by_id(&txn, id)
                    .map_err(|e| {
                    StoreError::Io(format!("get_by_id: {e}"))
                })? {
                    Some(ev) => {
                        let owned = ev.into_owned();
                        let fk = freshness_key_from_event(&owned);
                        let kind = owned.kind.as_u16() as u32;
                        let tags: Vec<Vec<String>> =
                            owned.tags.iter().map(|t| t.clone().to_vec()).collect();
                        (fk, kind, Some(tags))
                    }
                    None => (None, 0, None),
                };
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
                provenance::delete(
                    inner.provenance,
                    inner.relay_index,
                    inner.relay_kind,
                    &mut txn,
                    id,
                    kind,
                )?;
                lru_delete(inner, &mut txn, id)?;
                // #1811: drop the expired event's FTS rows (doc-key-driven).
                fts::fts_remove_by_id(inner, &mut txn, id)?;
                // Bug-2 fix: delete stale replaceable_freshness row.
                if let Some(fk) = freshness_key {
                    inner
                        .lmdb
                        .delete_freshness(&mut txn, &fk)
                        .map_err(|e| StoreError::Io(format!("expiry delete_freshness: {e}")))?;
                }
                // Issue #1519: decrement interaction-counter for expired event.
                if let Some(ref tags) = event_tags {
                    super::interaction_counters::apply_on_remove(
                        inner.interaction_counters,
                        &inner.reference_classifier,
                        &mut txn,
                        kind,
                        tags,
                    )?;
                }
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
                // ADR-0058 §3: emit Nip40Expiry log entry inside this txn (D4).
                ingest_log::append_deleted(
                    inner.ingest_log,
                    inner.ingest_meta,
                    &mut txn,
                    id,
                    *id,
                    DeleteReason::Nip40Expiry,
                    now_secs * 1000,
                    inner.map_size,
                    inner.max_readers,
                    &retention_claims,
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
                    let (k, val) = entry.map_err(|e| StoreError::Io(format!("lru entry: {e}")))?;
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
                // K3 Stage D3 backstop: per guard, the lowest `created_at` of an
                // evicted event the guard matches at/below its `covered_through`.
                // Lowered into the SAME `txn` below, so the ledger update commits
                // atomically with the deletes.
                let mut min_evicted_covered: Vec<Option<u64>> = vec![None; guards.len()];
                for (_, id) in &candidates {
                    // Load the event before deletion to capture expiry and
                    // coordinate info needed for O(1) secondary-index cleanup
                    // (expiry-index + replaceable_freshness — Bug-2 fix) AND the
                    // guard-relevant fields (author/kind/created_at/tags) for the
                    // K3 Stage D3 backstop.
                    // Use the write txn for the read (heed RwTxn derefs to
                    // RoTxn; mirrors the pattern in delete.rs:by_ids).
                    let (expiry, freshness_key, guard_fields, kind, event_tags) = match inner
                        .lmdb
                        .get_event_by_id(&txn, id)
                        .map_err(|e| StoreError::Io(format!("get_by_id: {e}")))?
                    {
                        None => (None, None, None, 0, None),
                        Some(ev) => {
                            let owned = ev.into_owned();
                            let exp = owned.tags.expiration().map(|ts| ts.as_secs());
                            let fk = freshness_key_from_event(&owned);
                            let kind = owned.kind.as_u16() as u32;
                            let tags: Vec<Vec<String>> =
                                owned.tags.iter().map(|t| t.clone().to_vec()).collect();
                            let gf = if guards.is_empty() {
                                None
                            } else {
                                Some((
                                    owned.pubkey.to_hex(),
                                    kind,
                                    owned.created_at.as_secs(),
                                    tags.clone(),
                                ))
                            };
                            (exp, fk, gf, kind, Some(tags))
                        }
                    };
                    let f = Filter::new().id(nostr::EventId::from_slice(id)
                        .map_err(|e| StoreError::Encoding(format!("id: {e}")))?);
                    inner
                        .lmdb
                        .delete(&mut txn, f)
                        .map_err(|e| StoreError::Io(format!("lru evict del: {e}")))?;
                    provenance::delete(
                        inner.provenance,
                        inner.relay_index,
                        inner.relay_kind,
                        &mut txn,
                        id,
                        kind,
                    )?;
                    lru_delete(inner, &mut txn, id)?;
                    // #1811: drop the LRU-evicted event's FTS rows (doc-key-driven).
                    fts::fts_remove_by_id(inner, &mut txn, id)?;
                    // V-118: clean expiry-index using the known expiry timestamp.
                    expiry_index_delete_exact(inner, &mut txn, expiry, id)?;
                    // Issue #1519: decrement interaction-counter for evicted event.
                    if let Some(ref tags) = event_tags {
                        super::interaction_counters::apply_on_remove(
                            inner.interaction_counters,
                            &inner.reference_classifier,
                            &mut txn,
                            kind,
                            tags,
                        )?;
                    }
                    // Bug-2 fix: delete stale replaceable_freshness row so a
                    // re-fetch after eviction is not wrongly skipped.
                    if let Some(fk) = freshness_key {
                        inner.lmdb.delete_freshness(&mut txn, &fk).map_err(|e| {
                            StoreError::Io(format!("lru evict delete_freshness: {e}"))
                        })?;
                    }
                    // K3 Stage D3: record the evicted-below-floor timestamp per
                    // matching guard so the row is lowered before this txn commits.
                    if let Some((author, kind, created_at, tags)) = guard_fields {
                        let id_hex = nostr::EventId::from_slice(id)
                            .map(|e| e.to_hex())
                            .unwrap_or_default();
                        for (gi, guard) in guards.iter().enumerate() {
                            if created_at <= guard.covered_through
                                && (guard.matches)(&id_hex, &author, kind, created_at, &tags)
                            {
                                let slot = &mut min_evicted_covered[gi];
                                *slot = Some(slot.map_or(created_at, |m| m.min(created_at)));
                            }
                        }
                    }
                    report.lru_evicted += 1;
                    if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
                        coverage::lower_guards_in_txn(
                            inner,
                            &mut txn,
                            guards,
                            &min_evicted_covered,
                        )?;
                        txn.commit()
                            .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
                        return finish(start, report);
                    }
                }
                // Atomic with the deletes: lower every covered row that lost a
                // below-floor event this pass, then commit them together.
                coverage::lower_guards_in_txn(inner, &mut txn, guards, &min_evicted_covered)?;
                txn.commit()
                    .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
            }
        }
    }

    // ── Phase 3 + 3b: Purge old tombstones and address tombstones ────────────
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
    //
    // Implementation extracted to `gc_tombstones.rs` for the 500-LOC cap.
    let last_purge = inner.gc_last_tombstone_purge_secs.load(Ordering::Relaxed);
    let purge_due = now_secs.saturating_sub(last_purge) >= GC_TOMBSTONE_PURGE_INTERVAL_SECS;

    if purge_due {
        // Mark the purge as starting now so that even if we bail early (budget)
        // we do not re-enter the expensive scan in the same hour.
        inner
            .gc_last_tombstone_purge_secs
            .store(now_secs, Ordering::Relaxed);
        gc_tombstones::purge_tombstones(inner, now_secs, &mut report)?;
    }

    finish(start, report)
}

#[inline]
fn finish(start: std::time::Instant, mut report: GcReport) -> Result<GcReport, StoreError> {
    report.duration_ms = start.elapsed().as_millis() as u32;
    Ok(report)
}
