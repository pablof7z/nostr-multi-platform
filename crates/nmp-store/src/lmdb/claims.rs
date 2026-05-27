//! Claims / view-cover budgets for the LMDB backend.
//!
//! Mirrors `mem/gc.rs` semantics exactly:
//!   * `register_view_cover` records the per-claimer ceiling (default 1000).
//!   * `claim` enforces per-view + global (BTreeSet union, not sum) ceilings,
//!     dedups intra-call repeats, idempotent re-claims.
//!   * `release` removes both pin set and budget for a claimer.
//!
//! Storage layout:
//!   * `nmp-claims-budget` — claimer (8 bytes BE u64) → ceiling (8 bytes BE u64).
//!   * `nmp-claims`        — claimer (8 bytes BE u64) || event_id (32 bytes) → empty.

use std::collections::BTreeSet;
use std::sync::Arc;

use heed::types::Bytes;
use heed::{Database, RwTxn};

use super::Inner;
use crate::types::{ClaimerId, EventId};
use crate::StoreError;

/// Mirrored from `mem/mod.rs:36-37`.
const DEFAULT_VIEW_CEILING: usize = 1_000;
const MAX_PINNED_TOTAL: usize = 20_000;

fn claimer_key(c: ClaimerId) -> [u8; 8] {
    c.0.to_be_bytes()
}

fn put_budget(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    c: ClaimerId,
    n: usize,
) -> Result<(), StoreError> {
    let k = claimer_key(c);
    let v = (n as u64).to_be_bytes();
    db.put(txn, &k, &v)
        .map_err(|e| StoreError::Io(format!("budget put: {e}")))
}

fn get_budget_rw(
    db: Database<Bytes, Bytes>,
    txn: &RwTxn,
    c: ClaimerId,
) -> Result<Option<usize>, StoreError> {
    let k = claimer_key(c);
    match db
        .get(txn, &k)
        .map_err(|e| StoreError::Io(format!("budget get: {e}")))?
    {
        Some(v) if v.len() == 8 => {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(v);
            Ok(Some(u64::from_be_bytes(arr) as usize))
        }
        _ => Ok(None),
    }
}

fn delete_budget(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    c: ClaimerId,
) -> Result<(), StoreError> {
    let k = claimer_key(c);
    db.delete(txn, &k)
        .map_err(|e| StoreError::Io(format!("budget del: {e}")))?;
    Ok(())
}

fn make_claim_key(c: ClaimerId, id: &EventId) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + 32);
    out.extend_from_slice(&claimer_key(c));
    out.extend_from_slice(id);
    out
}

fn claimer_prefix(c: ClaimerId) -> [u8; 8] {
    claimer_key(c)
}

pub(super) fn register_view_cover(
    inner: &Arc<Inner>,
    claimer: ClaimerId,
    cover_budget: usize,
) -> Result<(), StoreError> {
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    put_budget(inner.claims_budget, &mut txn, claimer, cover_budget)?;
    txn.commit()
        .map_err(|e| StoreError::Io(format!("commit: {e}")))
}

pub(super) fn claim(
    inner: &Arc<Inner>,
    claimer: ClaimerId,
    ids: &[EventId],
) -> Result<(), StoreError> {
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    let ceiling =
        get_budget_rw(inner.claims_budget, &txn, claimer)?.unwrap_or(DEFAULT_VIEW_CEILING);

    // Load existing claimer set.
    let prefix = claimer_prefix(claimer);
    let mut existing: BTreeSet<EventId> = BTreeSet::new();
    for entry in inner
        .claims
        .prefix_iter(&txn, &prefix)
        .map_err(|e| StoreError::Io(format!("claims iter: {e}")))?
    {
        let (k, _) = entry.map_err(|e| StoreError::Io(format!("claims step: {e}")))?;
        if k.len() == 40 {
            let mut id = [0u8; 32];
            id.copy_from_slice(&k[8..]);
            existing.insert(id);
        }
    }

    // Intra-call dedup; filter already-claimed.
    let new_ids: BTreeSet<EventId> = ids
        .iter()
        .filter(|id| !existing.contains(*id))
        .copied()
        .collect();

    let requested_for_claimer = existing.len() + new_ids.len();
    if requested_for_claimer > ceiling {
        return Err(StoreError::OverPinned {
            claimer,
            requested: requested_for_claimer,
            ceiling,
        });
    }

    // Global pinned ceiling via UNION across all claimers.
    let mut global: BTreeSet<EventId> = BTreeSet::new();
    for entry in inner
        .claims
        .iter(&txn)
        .map_err(|e| StoreError::Io(format!("claims iter: {e}")))?
    {
        let (k, _) = entry.map_err(|e| StoreError::Io(format!("claims step: {e}")))?;
        if k.len() == 40 {
            let mut id = [0u8; 32];
            id.copy_from_slice(&k[8..]);
            global.insert(id);
        }
    }
    let global_new = new_ids.iter().filter(|id| !global.contains(*id)).count();
    let requested_global = global.len() + global_new;
    if requested_global > MAX_PINNED_TOTAL {
        return Err(StoreError::OverPinned {
            claimer,
            requested: requested_global,
            ceiling: MAX_PINNED_TOTAL,
        });
    }

    // Apply.
    for id in &new_ids {
        let k = make_claim_key(claimer, id);
        inner
            .claims
            .put(&mut txn, &k, b"")
            .map_err(|e| StoreError::Io(format!("claim put: {e}")))?;
    }
    txn.commit()
        .map_err(|e| StoreError::Io(format!("commit: {e}")))
}

pub(super) fn release(inner: &Arc<Inner>, claimer: ClaimerId) -> Result<(), StoreError> {
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    let prefix = claimer_prefix(claimer);
    // heed's `delete_range` over the prefix requires bounds; use prefix_iter +
    // collect-then-delete to stay within the safe heed API surface.
    let keys: Vec<Vec<u8>> = {
        let iter = inner
            .claims
            .prefix_iter(&txn, &prefix)
            .map_err(|e| StoreError::Io(format!("claims iter: {e}")))?;
        let mut out = Vec::new();
        for entry in iter {
            let (k, _) = entry.map_err(|e| StoreError::Io(format!("claims step: {e}")))?;
            out.push(k.to_vec());
        }
        out
    };
    for k in keys {
        inner
            .claims
            .delete(&mut txn, &k)
            .map_err(|e| StoreError::Io(format!("claim del: {e}")))?;
    }
    delete_budget(inner.claims_budget, &mut txn, claimer)?;
    txn.commit()
        .map_err(|e| StoreError::Io(format!("commit: {e}")))
}
