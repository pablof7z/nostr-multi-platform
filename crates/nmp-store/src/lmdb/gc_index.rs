//! Secondary-index maintenance helpers for the LMDB backend.
//!
//! Extracted from `gc.rs` to keep that file under the 500-LOC hard cap
//! (AGENTS.md). These are the low-level put/delete primitives that every
//! write path (insert, delete, kind:5, GC) calls to keep the NMP-side
//! secondary indexes coherent with the main event store:
//!
//!   * **LRU access index** (`nmp-lru-access`) — `lru_stamp` / `lru_delete`.
//!   * **Expiry index** (`nmp-expiry-index`, V-118 / #1097) —
//!     `expiry_index_key` / `expiry_index_put` / `expiry_index_delete_exact`.
//!   * **Replaceable-freshness key derivation** — `freshness_key_from_event`,
//!     used by deletion / eviction paths to drop stale TTL rows.
//!
//! All functions operate inside a caller-supplied `RwTxn` so the secondary
//! write commits atomically with the event write (D6).
//!
//! These remain reachable as `gc::<name>` because `gc.rs` re-exports them
//! (`pub(super) use gc_index::*`), so existing call sites are unchanged.

#![cfg(feature = "lmdb-backend")]

use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::Inner;
use crate::types::EventId;
use crate::StoreError;

/// Build a `ReplaceableKey` from a decoded event if it is replaceable or
/// addressable.  Returns `None` for regular (non-replaceable) event kinds.
///
/// Used by the LRU eviction + expiry-reap phases to delete stale
/// `replaceable_freshness` entries so a re-fetch after eviction is not
/// wrongly skipped (Bug-2 fix).
pub(super) fn freshness_key_from_event(
    event: &nostr::Event,
) -> Option<nmp_nostr_lmdb::ReplaceableKey> {
    let pubkey: [u8; 32] = event.pubkey.to_bytes();
    let kind = u32::from(event.kind.as_u16());
    if event.kind.is_addressable() {
        let d_tag = event.tags.identifier().unwrap_or_default().to_string();
        Some(nmp_nostr_lmdb::ReplaceableKey::Parameterized {
            kind,
            pubkey,
            d_tag,
        })
    } else if event.kind.is_replaceable() {
        Some(nmp_nostr_lmdb::ReplaceableKey::Regular { kind, pubkey })
    } else {
        None
    }
}

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
        .map_err(|e| super::open_error::classify_heed_err(e, inner.map_size, inner.max_readers))
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
