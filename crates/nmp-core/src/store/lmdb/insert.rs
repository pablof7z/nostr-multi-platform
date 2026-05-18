//! §7.1 insert invariants for the LMDB backend.
//!
//! Wraps `nmp_nostr_lmdb::Lmdb::save_event_with_txn` with the pre/post
//! compensation defined in ADR-0012. Every step runs inside a single
//! `heed::RwTxn` so the event write + NMP-side secondaries either all
//! land or all roll back (D6 atomicity).

use std::sync::Arc;

use heed::RwTxn;
use nmp_nostr_lmdb::SaveEventStatus;
use nostr_database::FlatBufferBuilder;
use nostr_database::RejectedReason;

use super::{conv, provenance, tombstones, Inner};
use crate::store::types::{
    EventId, InsertOutcome, RawEvent, RejectReason, RelayUrl, TombstoneOrigin,
};
use crate::store::StoreError;

pub(super) fn insert(
    inner: &Arc<Inner>,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> Result<InsertOutcome, StoreError> {
    // 1. Structural validation.
    if !event.is_structurally_valid() {
        return Ok(InsertOutcome::Rejected {
            id: event.id_bytes(),
            reason: RejectReason::Malformed("invalid id/pubkey/sig length".into()),
        });
    }

    // 2. Ephemeral kinds — never stored.
    if event.is_ephemeral() {
        return Ok(InsertOutcome::Ephemeral { id: event.id_bytes() });
    }

    // 3. NIP-40 expiration on arrival.
    if let Some(exp) = event.expiration() {
        let now_secs = received_at_ms / 1000;
        if exp <= now_secs {
            // Open a write txn just to mark the tombstone; matches Mem's
            // behavior of not storing the event AND not creating a row, but
            // we DO record an NIP40Expiry tombstone for symmetry with the
            // GC-reaper path. Mem does not store one here either — keep
            // parity: no tombstone on ExpiredOnArrival.
            return Ok(InsertOutcome::Rejected {
                id: event.id_bytes(),
                reason: RejectReason::ExpiredOnArrival,
            });
        }
    }

    let id_bytes = event.id_bytes();

    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;

    // 4. Per-id tombstone check (NMP-side).
    if let Some(tomb) = tombstones::get(inner.tombstones, &txn, &id_bytes)? {
        let applies = match tomb.origin {
            TombstoneOrigin::Kind5 => {
                tomb.deleter_pubkey
                    .as_ref()
                    .map(|dp| hex_eq(dp, &event.pubkey))
                    .unwrap_or(false)
            }
            TombstoneOrigin::NIP40Expiry | TombstoneOrigin::AdminPurge => true,
        };
        if applies {
            txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))?;
            return Ok(InsertOutcome::Tombstoned {
                id: id_bytes,
                kind5_event_id: tomb.kind5_event_id,
                origin: tomb.origin,
            });
        }
        // Foreign pre-tombstone — drop and proceed (parity with mem/insert.rs:74-76).
        tombstones::delete(inner.tombstones, &mut txn, &id_bytes)?;
        // Also clear the fork's `is_deleted` mark via... actually the fork has
        // no public `clear_deleted`. Workaround: we never wrote the fork's
        // mark for foreign pre-tombstones (we only mark when applies=true).
    }

    // 5. Address tombstone check (param-replaceable).
    if event.is_param_replaceable() {
        if let Some(d) = event.d_tag() {
            let key = tombstones::addr_key(event.kind, &event.pubkey, &d);
            if let Some(tomb) = tombstones::get_addr(inner.addr_tombstones, &txn, &key)? {
                if tomb.deleted_at >= event.created_at {
                    txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))?;
                    return Ok(InsertOutcome::Tombstoned {
                        id: id_bytes,
                        kind5_event_id: tomb.kind5_event_id,
                        origin: tomb.origin,
                    });
                }
            }
        }
    }

    // 6. Kind:5 — special handling, then fall through to fork's normal save.
    if event.kind == 5 {
        let outcome = handle_kind5(inner, &mut txn, event, source, received_at_ms)?;
        txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))?;
        return Ok(outcome);
    }

    // 7. Replaceable / addressable — pre-query existing for outcome typing.
    let pre_existing_id: Option<EventId> = pre_query_existing(inner, &txn, &event)?;

    // 8. Convert to nostr::Event for the fork.
    let nostr_ev = conv::raw_to_nostr(&event)?;

    // 9. Delegate to fork's save_event_with_txn (atomic event + index update).
    let mut fbb = FlatBufferBuilder::with_capacity(4096);
    let status = inner
        .lmdb
        .save_event_with_txn(&mut txn, &mut fbb, &nostr_ev)
        .map_err(|e| StoreError::Io(format!("save_event_with_txn: {e}")))?;

    // 10. Map fork status → InsertOutcome.
    let outcome = match status {
        SaveEventStatus::Success => {
            // Provenance upsert.
            let count = provenance::upsert(
                inner.provenance,
                &mut txn,
                &id_bytes,
                source.clone(),
                received_at_ms,
            )?;
            if let Some(replaced_id) = pre_existing_id {
                // Replaced — also drop the replaced event's provenance.
                provenance::delete(inner.provenance, &mut txn, &replaced_id)?;
                InsertOutcome::Replaced { new_id: id_bytes, replaced_id }
            } else {
                let _ = count; // (semi-)unused for Inserted variant; kept for symmetry.
                InsertOutcome::Inserted { id: id_bytes, sources_after: count }
            }
        }
        SaveEventStatus::Rejected(RejectedReason::Duplicate) => {
            let count = provenance::upsert(
                inner.provenance,
                &mut txn,
                &id_bytes,
                source.clone(),
                received_at_ms,
            )?;
            InsertOutcome::Duplicate { id: id_bytes, sources_after: count }
        }
        SaveEventStatus::Rejected(RejectedReason::Replaced) => {
            // The fork's "Replaced" rejection = incoming is older than what
            // we have — Mem's `Superseded { id, current_id }`. The
            // `current_id` is whatever pre_query found.
            InsertOutcome::Superseded {
                id: id_bytes,
                current_id: pre_existing_id.unwrap_or(id_bytes),
            }
        }
        SaveEventStatus::Rejected(RejectedReason::Deleted) => {
            // Look up tombstone metadata.
            let tomb = tombstones::get(inner.tombstones, &txn, &id_bytes)?;
            let (kind5_event_id, origin) = match tomb {
                Some(t) => (t.kind5_event_id, t.origin),
                None => (None, TombstoneOrigin::AdminPurge),
            };
            InsertOutcome::Tombstoned { id: id_bytes, kind5_event_id, origin }
        }
        SaveEventStatus::Rejected(RejectedReason::Ephemeral) => {
            // Unreachable — pre-shortcircuit handled it. Defensive map.
            InsertOutcome::Ephemeral { id: id_bytes }
        }
        SaveEventStatus::Rejected(RejectedReason::InvalidDelete) => {
            // Should never fire — we pre-filter foreign-author tags in kind:5
            // path. Map to Rejected/Malformed for defensive safety.
            InsertOutcome::Rejected {
                id: id_bytes,
                reason: RejectReason::Malformed("fork InvalidDelete".into()),
            }
        }
        // Forward-compat: any future RejectedReason variants map to Malformed.
        SaveEventStatus::Rejected(other) => InsertOutcome::Rejected {
            id: id_bytes,
            reason: RejectReason::Malformed(format!("fork rejected: {other:?}")),
        },
    };

    txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    Ok(outcome)
}

/// Look up the existing event id for a replaceable / addressable so the
/// outcome can carry `replaced_id` / `current_id`. Returns `None` for
/// non-replaceable kinds or when nothing matches.
fn pre_query_existing(
    inner: &Arc<Inner>,
    txn: &heed::RwTxn,
    event: &RawEvent,
) -> Result<Option<EventId>, StoreError> {
    use nostr::prelude::*;
    if event.is_replaceable() {
        let pk_bytes = event.pubkey_bytes();
        let pk = match PublicKey::from_slice(&pk_bytes) {
            Ok(pk) => pk,
            Err(_) => return Ok(None),
        };
        let kind = Kind::from(event.kind as u16);
        match inner
            .lmdb
            .find_replaceable_event(txn, &pk, kind)
            .map_err(|e| StoreError::Io(format!("find_replaceable: {e}")))?
        {
            Some(ev) => {
                let mut id = [0u8; 32];
                id.copy_from_slice(ev.id);
                Ok(Some(id))
            }
            None => Ok(None),
        }
    } else if event.is_param_replaceable() {
        let d = match event.d_tag() {
            Some(d) => d,
            None => return Ok(None),
        };
        let pk_bytes = event.pubkey_bytes();
        let pk = match PublicKey::from_slice(&pk_bytes) {
            Ok(pk) => pk,
            Err(_) => return Ok(None),
        };
        let kind = Kind::from(event.kind as u16);
        let d_str = String::from_utf8_lossy(&d).into_owned();
        let coord = Coordinate::new(kind, pk).identifier(d_str);
        match inner
            .lmdb
            .find_addressable_event(txn, &coord)
            .map_err(|e| StoreError::Io(format!("find_addressable: {e}")))?
        {
            Some(ev) => {
                let mut id = [0u8; 32];
                id.copy_from_slice(ev.id);
                Ok(Some(id))
            }
            None => Ok(None),
        }
    } else {
        Ok(None)
    }
}

/// Mem-parity kind:5 handling.
///
/// Walks `e`-tags and `a`-tags, removes self-deleted targets (foreign
/// targets are silently skipped — matching `mem/insert.rs:271 continue`),
/// writes tombstones, then stores the kind:5 event itself. Crucially we do
/// NOT pass the foreign-tag bits to the fork's `save_event_with_txn` — we
/// pre-filter and store the kind:5 directly via `Lmdb::store` so the fork's
/// `handle_deletion_event` (which would reject the whole event on a foreign
/// target) never sees them.
fn handle_kind5(
    inner: &Arc<Inner>,
    txn: &mut RwTxn,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> Result<InsertOutcome, StoreError> {
    use nostr::prelude::*;

    let kind5_id = event.id_bytes();
    let kind5_pubkey = event.pubkey_bytes();
    let kind5_at = event.created_at;

    // Process `e`-tag deletes — self-deletes only.
    for target_hex in event.e_tags() {
        let target_id_bytes = RawEvent::hex_to_bytes32_owned(&target_hex);
        // Author check: load target via fork; skip if author mismatch.
        let target_is_self = match inner
            .lmdb
            .get_event_by_id(txn, &target_id_bytes)
            .map_err(|e| StoreError::Io(format!("k5 get: {e}")))?
        {
            Some(target) => target.pubkey == kind5_pubkey.as_slice(),
            None => true, // No target stored — record tombstone for future arrivals.
        };
        if !target_is_self {
            continue;
        }

        // Tombstone write (max-merge), then mark fork's deleted_ids + remove
        // event from fork if present.
        let row = tombstones::kind5_row(
            target_id_bytes,
            kind5_id,
            kind5_pubkey,
            kind5_at,
            source,
        );
        tombstones::merge_per_id(inner.tombstones, txn, &target_id_bytes, row)?;

        // Mark deleted_ids on the fork (its `save_event_with_txn` pre-check
        // for re-deliveries depends on this).
        if let Ok(target_event_id) = EventId::from_slice(&target_id_bytes) {
            inner
                .lmdb
                .mark_deleted(txn, &target_event_id)
                .map_err(|e| StoreError::Io(format!("k5 mark_deleted: {e}")))?;
        }

        // Remove the target's primary + indexes if it exists.
        if inner
            .lmdb
            .get_event_by_id(txn, &target_id_bytes)
            .map_err(|e| StoreError::Io(format!("k5 get2: {e}")))?
            .is_some()
        {
            // The fork doesn't expose a single-id deletion; emulate by
            // delete-by-filter on the id.
            let filter = nostr::Filter::new().id(EventId::from_slice(&target_id_bytes).map_err(
                |e| StoreError::Encoding(format!("k5 id: {e}")),
            )?);
            inner
                .lmdb
                .delete(txn, filter)
                .map_err(|e| StoreError::Io(format!("k5 delete: {e}")))?;
            // Also drop NMP-side provenance.
            provenance::delete(inner.provenance, txn, &target_id_bytes)?;
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
        let Ok(tgt_kind) = tgt_kind_str.parse::<u32>() else { continue };

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

        // Remove all matching events ≤ kind5.created_at via the fork.
        if let Ok(pk) = PublicKey::from_slice(&kind5_pubkey) {
            let coord = Coordinate::new(Kind::from(tgt_kind as u16), pk)
                .identifier(tgt_dtag.to_string());
            if coord.kind.is_addressable() {
                inner
                    .lmdb
                    .remove_addressable(txn, &coord, Timestamp::from_secs(kind5_at))
                    .map_err(|e| StoreError::Io(format!("k5 remove_addressable: {e}")))?;
            } else if coord.kind.is_replaceable() {
                inner
                    .lmdb
                    .remove_replaceable(txn, &coord, Timestamp::from_secs(kind5_at))
                    .map_err(|e| StoreError::Io(format!("k5 remove_replaceable: {e}")))?;
            }
            // Mark coordinate-deleted in the fork's index for future events.
            inner
                .lmdb
                .mark_coordinate_deleted(txn, &coord.borrow(), Timestamp::from_secs(kind5_at))
                .map_err(|e| StoreError::Io(format!("k5 mark_coord: {e}")))?;
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
            txn,
            &kind5_id,
            source.clone(),
            received_at_ms,
        )?;
        return Ok(InsertOutcome::Duplicate { id: kind5_id, sources_after: count });
    }
    inner
        .lmdb
        .store(txn, &mut fbb, &nostr_ev)
        .map_err(|e| StoreError::Io(format!("k5 store: {e}")))?;
    let count = provenance::upsert(
        inner.provenance,
        txn,
        &kind5_id,
        source.clone(),
        received_at_ms,
    )?;
    Ok(InsertOutcome::Inserted { id: kind5_id, sources_after: count })
}

/// Hex-eq for the deleter_pubkey check. `dp` is `[u8; 32]`; `pubkey_hex`
/// is lowercase hex.
fn hex_eq(dp: &[u8; 32], pubkey_hex: &str) -> bool {
    if pubkey_hex.len() != 64 {
        return false;
    }
    let parsed = RawEvent::hex_to_bytes32_owned(pubkey_hex);
    &parsed == dp
}

// (delete_by_filter moved to `delete.rs` so this file fits the 500-LOC cap.)
