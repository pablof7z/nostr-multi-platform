//! NIP-09 (kind:5) deletion handling for the LMDB backend.
//!
//! Extracted from `insert.rs` to keep that file under the 500-LOC hard cap
//! (AGENTS.md). `handle_kind5` is called by `insert::insert` for kind:5
//! events; everything else in the insert pipeline lives in `insert.rs`.
//!
//! ## Semantics (Mem-parity, ADR-0012)
//!
//! Walks `e`-tags and `a`-tags, removes self-deleted targets (foreign targets
//! are silently skipped — matching `mem/insert.rs:271 continue`), writes
//! NMP tombstones, then stores the kind:5 event itself.  Crucially we do NOT
//! pass the foreign-tag bits to the fork's `save_event_with_txn` — we
//! pre-filter and store the kind:5 directly via `Lmdb::store` so the fork's
//! `handle_deletion_event` (which would reject the whole event on a foreign
//! target, or poison `deleted_ids`) never sees them.
//!
//! Every removal path also cleans the NMP-side secondary indexes
//! (`nmp-provenance`, `nmp-lru-access`, `nmp-expiry-index`) and the
//! `replaceable_freshness` row for the removed coordinate so no dangling
//! secondary survives a deletion.

#![cfg(feature = "lmdb-backend")]

use std::sync::Arc;

use heed::RwTxn;
use nostr_database::FlatBufferBuilder;

use super::{conv, gc, ingest_log, provenance, tombstones, Inner};
use crate::ingest_log::DeleteReason;
use crate::types::{InsertOutcome, RawEvent, RelayUrl};
use crate::StoreError;

/// Mem-parity kind:5 handling.
pub(super) fn handle_kind5(
    inner: &Arc<Inner>,
    txn: &mut RwTxn,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> Result<InsertOutcome, StoreError> {
    use nostr::prelude::*;

    // handle_kind5 is only called after is_structurally_valid() passes.
    let kind5_id = event.id_bytes().expect("passed is_structurally_valid");
    let kind5_pubkey = event.pubkey_bytes().expect("passed is_structurally_valid");
    let kind5_at = event.created_at;

    // Process `e`-tag deletes — self-deletes only.
    for target_hex in event.e_tags() {
        // target_hex is from an e-tag value — may be malformed. Skip if undecidable.
        let Some(target_id_bytes) = RawEvent::hex_to_bytes32_owned(&target_hex) else {
            continue;
        };
        // Author check: load target via fork; capture expiry for O(1) index cleanup.
        let (target_is_self, target_stored, target_expiry, target_kind, target_tags) = match inner
            .lmdb
            .get_event_by_id(txn, &target_id_bytes)
            .map_err(|e| StoreError::Io(format!("k5 get: {e}")))?
        {
            Some(target) => {
                let owned = target.into_owned();
                let is_self = owned.pubkey.as_bytes().as_slice() == kind5_pubkey.as_slice();
                let expiry = owned.tags.expiration().map(|ts| ts.as_secs());
                let kind = owned.kind.as_u16() as u32;
                let tags: Vec<Vec<String>> = owned.tags.iter().map(|t| t.clone().to_vec()).collect();
                (is_self, true, expiry, kind, Some(tags))
            }
            None => (true, false, None, 0, None), // Not stored — tombstone for future arrivals.
        };
        if !target_is_self {
            continue;
        }

        // Tombstone write (max-merge). We deliberately do NOT call the fork's
        // `mark_deleted` here: when the target is not yet stored we default
        // `target_is_self = true` (we have to record SOMETHING in case it
        // arrives later), but a foreign kind:5 referencing Alice's still-
        // unfetched event must NOT poison the fork's `deleted_ids` set —
        // otherwise step 4 will drop the NMP tombstone on Alice's arrival
        // (foreign pre-tombstone path) only to have the fork re-reject the
        // event with `Deleted`, diverging from Mem's `Inserted` outcome.
        // Re-delivery rejection of legitimate self-deletes is handled by
        // the NMP per-id tombstone check in step 4 (see `applies` logic).
        // Verified by reading fork's `save_event_with_txn` (mod.rs:461):
        // `is_deleted` reads ONLY `deleted_ids`, which is now never written
        // by NMP on this path.
        let row = tombstones::kind5_row(target_id_bytes, kind5_id, kind5_pubkey, kind5_at, source);
        tombstones::merge_per_id(inner.tombstones, txn, &target_id_bytes, row)?;

        // Remove the target's primary + indexes if it was present.
        if target_stored {
            // The fork doesn't expose a single-id deletion; emulate by
            // delete-by-filter on the id.
            let filter = nostr::Filter::new().id(EventId::from_slice(&target_id_bytes)
                .map_err(|e| StoreError::Encoding(format!("k5 id: {e}")))?);
            inner
                .lmdb
                .delete(txn, filter)
                .map_err(|e| StoreError::Io(format!("k5 delete: {e}")))?;
            // Also drop NMP-side provenance and LRU entry.
            provenance::delete(
                inner.provenance,
                inner.relay_index,
                inner.relay_kind,
                txn,
                &target_id_bytes,
                target_kind,
            )?;
            gc::lru_delete(inner, txn, &target_id_bytes)?;
            // V-118: O(1) expiry-index cleanup using the known expiry timestamp.
            gc::expiry_index_delete_exact(inner, txn, target_expiry, &target_id_bytes)?;
            // Issue #1519: decrement interaction-counter for the removed event.
            if inner.interaction_counters_usable {
                if let Some(ref tags) = target_tags {
                    super::interaction_counters::apply_on_remove(
                        inner.interaction_counters,
                        txn,
                        target_kind,
                        tags,
                    )?;
                }
            }
            // ADR-0058 §3: emit Deleted(Nip09) for each self-deleted target.
            ingest_log::append_deleted(
                inner.ingest_log,
                inner.ingest_meta,
                txn,
                &kind5_id,
                target_id_bytes,
                DeleteReason::Nip09,
                received_at_ms,
            )?;
        }
    }

    // Process `a`-tag deletes — self only.
    for addr in event.a_tags() {
        let parts: Vec<&str> = addr.splitn(3, ':').collect();
        if parts.len() < 3 {
            continue;
        }
        let (tgt_kind_str, tgt_pk_hex, tgt_dtag) = (parts[0], parts[1], parts[2]);
        if tgt_pk_hex != event.pubkey {
            continue;
        }
        let Ok(tgt_kind) = tgt_kind_str.parse::<u32>() else {
            continue;
        };

        // Coordinate-tombstone for future arrivals (max-merge).
        let addr_key_bytes = tombstones::addr_key(tgt_kind, tgt_pk_hex, tgt_dtag.as_bytes());
        let addr_row = tombstones::kind5_row(
            [0u8; 32], // No primary id for an address-tombstone.
            kind5_id,
            kind5_pubkey,
            kind5_at,
            source,
        );
        tombstones::merge_addr(inner.addr_tombstones, txn, &addr_key_bytes, addr_row)?;

        // Remove all matching events ≤ kind5.created_at via the fork,
        // also cleaning up NMP-side secondaries (lru-access, provenance,
        // expiry-index) for each removed event id (Bug-1 fix).
        if let Ok(pk) = PublicKey::from_slice(&kind5_pubkey) {
            let coord =
                Coordinate::new(Kind::from(tgt_kind as u16), pk).identifier(tgt_dtag.to_string());
            if coord.kind.is_addressable() {
                // Pre-query the existing addressable event to get its id + expiry
                // so we can clean NMP-side indexes in O(1) without a post-scan.
                if let Some(existing) = inner
                    .lmdb
                    .find_addressable_event(txn, &coord)
                    .map_err(|e| StoreError::Io(format!("k5 find_addressable: {e}")))?
                {
                    let owned = existing.into_owned();
                    if owned.created_at <= Timestamp::from_secs(kind5_at) {
                        let mut existing_id = [0u8; 32];
                        existing_id.copy_from_slice(owned.id.as_bytes());
                        let existing_expiry = owned.tags.expiration().map(|ts| ts.as_secs());
                        let existing_kind = owned.kind.as_u16() as u32;
                        let existing_tags: Vec<Vec<String>> = owned.tags.iter().map(|t| t.clone().to_vec()).collect();
                        inner
                            .lmdb
                            .remove_addressable(txn, &coord, Timestamp::from_secs(kind5_at))
                            .map_err(|e| StoreError::Io(format!("k5 remove_addressable: {e}")))?;
                        // Clean NMP-side secondary indexes for the removed event.
                        provenance::delete(
                            inner.provenance,
                            inner.relay_index,
                            inner.relay_kind,
                            txn,
                            &existing_id,
                            tgt_kind,
                        )?;
                        gc::lru_delete(inner, txn, &existing_id)?;
                        gc::expiry_index_delete_exact(inner, txn, existing_expiry, &existing_id)?;
                        // Issue #1519: decrement interaction-counter for removed addressable.
                        if inner.interaction_counters_usable {
                            super::interaction_counters::apply_on_remove(
                                inner.interaction_counters,
                                txn,
                                existing_kind,
                                &existing_tags,
                            )?;
                        }
                        // ADR-0058 §3: emit Deleted(Nip09) for the a-tag addressable target.
                        ingest_log::append_deleted(
                            inner.ingest_log,
                            inner.ingest_meta,
                            txn,
                            &kind5_id,
                            existing_id,
                            DeleteReason::Nip09,
                            received_at_ms,
                        )?;
                    }
                }
                // Delete the replaceable_freshness row for this coordinate so stale
                // TTL data cannot cause a re-fetch to be wrongly skipped (Bug-2 fix).
                let freshness_key = nmp_nostr_lmdb::ReplaceableKey::Parameterized {
                    kind: tgt_kind,
                    pubkey: kind5_pubkey,
                    d_tag: tgt_dtag.to_string(),
                };
                inner
                    .lmdb
                    .delete_freshness(txn, &freshness_key)
                    .map_err(|e| StoreError::Io(format!("k5 delete_freshness: {e}")))?;
            } else if coord.kind.is_replaceable() {
                // Pre-query the existing replaceable event.
                if let Some(existing) = inner
                    .lmdb
                    .find_replaceable_event(txn, &pk, coord.kind)
                    .map_err(|e| StoreError::Io(format!("k5 find_replaceable: {e}")))?
                {
                    let owned = existing.into_owned();
                    if owned.created_at <= Timestamp::from_secs(kind5_at) {
                        let mut existing_id = [0u8; 32];
                        existing_id.copy_from_slice(owned.id.as_bytes());
                        let existing_expiry = owned.tags.expiration().map(|ts| ts.as_secs());
                        let existing_kind = owned.kind.as_u16() as u32;
                        let existing_tags: Vec<Vec<String>> = owned.tags.iter().map(|t| t.clone().to_vec()).collect();
                        inner
                            .lmdb
                            .remove_replaceable(txn, &coord, Timestamp::from_secs(kind5_at))
                            .map_err(|e| StoreError::Io(format!("k5 remove_replaceable: {e}")))?;
                        // Clean NMP-side secondary indexes for the removed event.
                        provenance::delete(
                            inner.provenance,
                            inner.relay_index,
                            inner.relay_kind,
                            txn,
                            &existing_id,
                            tgt_kind,
                        )?;
                        gc::lru_delete(inner, txn, &existing_id)?;
                        gc::expiry_index_delete_exact(inner, txn, existing_expiry, &existing_id)?;
                        // Issue #1519: decrement interaction-counter for removed replaceable.
                        if inner.interaction_counters_usable {
                            super::interaction_counters::apply_on_remove(
                                inner.interaction_counters,
                                txn,
                                existing_kind,
                                &existing_tags,
                            )?;
                        }
                        // ADR-0058 §3: emit Deleted(Nip09) for the a-tag replaceable target.
                        ingest_log::append_deleted(
                            inner.ingest_log,
                            inner.ingest_meta,
                            txn,
                            &kind5_id,
                            existing_id,
                            DeleteReason::Nip09,
                            received_at_ms,
                        )?;
                    }
                }
                // Delete the replaceable_freshness row (Bug-2 fix).
                let freshness_key = nmp_nostr_lmdb::ReplaceableKey::Regular {
                    kind: tgt_kind,
                    pubkey: kind5_pubkey,
                };
                inner
                    .lmdb
                    .delete_freshness(txn, &freshness_key)
                    .map_err(|e| StoreError::Io(format!("k5 delete_freshness: {e}")))?;
            }
            // Note: we deliberately skip the fork's `mark_coordinate_deleted`
            // for the same reason as `mark_deleted` above. Future-arrival
            // rejection (a-tag tombstone) is handled by the NMP addr-tombstone
            // check at step 5 — keeping the fork's `deleted_coordinates`
            // index out of it preserves parity with Mem (which has no such
            // index). Verified by reading fork's `save_event_with_txn`
            // (mod.rs:468): `when_is_coordinate_deleted` reads ONLY
            // `deleted_coordinates`, which NMP now never writes to.
        }
    }

    // Finally, store the kind:5 event itself via the fork's low-level `store`
    // (bypassing `save_event_with_txn`'s `handle_deletion_event` since we
    // already did the pre-filtering + author-respecting deletion above).
    let nostr_ev = conv::raw_to_nostr(&event)?;
    let mut fbb = FlatBufferBuilder::with_capacity(2048);

    // Double-check: don't re-store if duplicate.
    let already = inner
        .lmdb
        .has_event(txn, &kind5_id)
        .map_err(|e| StoreError::Io(format!("k5 has_event: {e}")))?;
    if already {
        let count = provenance::upsert(
            inner.provenance,
            inner.relay_index,
            inner.relay_kind,
            txn,
            &kind5_id,
            source.clone(),
            5,
            received_at_ms,
            inner.map_size,
            inner.max_readers,
        )?;
        return Ok(InsertOutcome::Duplicate {
            id: kind5_id,
            sources_after: count,
        });
    }
    inner
        .lmdb
        .store(txn, &mut fbb, &nostr_ev)
        // Bulk kind:5 event write — classify so `MDB_MAP_FULL` surfaces as the
        // typed `StoreError::MapFull` health variant (#1521), never a stringly `Io`.
        .map_err(|e| super::open_error::classify_store_err(e, inner.map_size, inner.max_readers))?;
    let count = provenance::upsert(
        inner.provenance,
        inner.relay_index,
        inner.relay_kind,
        txn,
        &kind5_id,
        source.clone(),
        5,
        received_at_ms,
        inner.map_size,
        inner.max_readers,
    )?;
    // Stamp LRU access for the newly stored kind:5 event.
    gc::lru_stamp(inner, txn, &kind5_id)?;
    // V-118: index the kind:5's own expiry if present (defensive — kind:5
    // events do not normally carry expiration tags, but keep the index honest).
    if let Some(exp) = event.expiration() {
        gc::expiry_index_put(inner, txn, exp, &kind5_id)?;
    }
    // Issue #1519: kind:5 is itself kind 5 — not a counter kind (1/6/7/9735),
    // so apply_on_insert is a no-op. We still call it for consistency so the
    // code path is uniform.
    if inner.interaction_counters_usable {
        super::interaction_counters::apply_on_insert(
            inner.interaction_counters,
            txn,
            event.kind,
            &event.tags,
        )?;
    }
    // ADR-0058 §3: emit Inserted log entry for the kind:5 event itself.
    ingest_log::append_inserted(
        inner.ingest_log,
        inner.ingest_meta,
        txn,
        &kind5_id,
        event,
        source,
        received_at_ms,
    )?;
    Ok(InsertOutcome::Inserted {
        id: kind5_id,
        sources_after: count,
    })
}
