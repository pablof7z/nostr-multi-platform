//! §7.1 insert invariants for the LMDB backend.
//!
//! Wraps `nmp_nostr_lmdb::Lmdb::save_event_with_txn` with the pre/post
//! compensation defined in ADR-0012. Every step runs inside a single
//! `heed::RwTxn` so the event write + NMP-side secondaries either all
//! land or all roll back (D6 atomicity).

use std::sync::Arc;

use nmp_nostr_lmdb::SaveEventStatus;
use nostr_database::FlatBufferBuilder;
use nostr_database::RejectedReason;

use super::{conv, gc, provenance, tombstones, Inner};
use crate::types::{EventId, InsertOutcome, RawEvent, RejectReason, RelayUrl, TombstoneOrigin};
use crate::StoreError;

pub(super) fn insert(
    inner: &Arc<Inner>,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> Result<InsertOutcome, StoreError> {
    // 1. Structural validation.
    // is_structurally_valid() now verifies hex chars, so any id_bytes()/pubkey_bytes()
    // call after this gate is guaranteed to return Some.
    if !event.is_structurally_valid() {
        // id may be malformed hex; callers of Rejected do not read the id field.
        let id = event.id_bytes().unwrap_or([0u8; 32]);
        return Ok(InsertOutcome::Rejected {
            id,
            reason: RejectReason::Malformed("invalid id/pubkey/sig length or non-hex".into()),
        });
    }

    // 2. Ephemeral kinds — never stored.
    if event.is_ephemeral() {
        return Ok(InsertOutcome::Ephemeral {
            id: event.id_bytes().expect("passed is_structurally_valid"),
        });
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
                id: event.id_bytes().expect("passed is_structurally_valid"),
                reason: RejectReason::ExpiredOnArrival,
            });
        }
    }

    let id_bytes = event.id_bytes().expect("passed is_structurally_valid");

    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;

    // 4. Per-id tombstone check (NMP-side).
    if let Some(tomb) = tombstones::get(inner.tombstones, &txn, &id_bytes)? {
        let applies = match tomb.origin {
            TombstoneOrigin::Kind5 => tomb
                .deleter_pubkey
                .as_ref()
                .map(|dp| hex_eq(dp, &event.pubkey))
                .unwrap_or(false),
            TombstoneOrigin::NIP40Expiry | TombstoneOrigin::AdminPurge => true,
        };
        if applies {
            txn.commit()
                .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
            return Ok(InsertOutcome::Tombstoned {
                id: id_bytes,
                kind5_event_id: tomb.kind5_event_id,
                origin: tomb.origin,
            });
        }
        // Foreign pre-tombstone — drop and proceed (parity with mem/insert.rs:74-76).
        tombstones::delete(inner.tombstones, &mut txn, &id_bytes)?;
        // No fork-side `clear_deleted` is needed: `handle_kind5` never calls
        // the fork's `mark_deleted` (see rationale in that fn), so the fork's
        // `deleted_ids` set stays empty for any id NMP wrote a tombstone for.
        // `save_event_with_txn`'s `is_deleted` pre-check is therefore a no-op
        // on this path.
    }

    // 5. Address tombstone check (param-replaceable).
    if event.is_param_replaceable() {
        if let Some(d) = event.d_tag() {
            let key = tombstones::addr_key(event.kind, &event.pubkey, &d);
            if let Some(tomb) = tombstones::get_addr(inner.addr_tombstones, &txn, &key)? {
                if tomb.deleted_at >= event.created_at {
                    txn.commit()
                        .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
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
        let outcome = super::insert_kind5::handle_kind5(inner, &mut txn, event, source, received_at_ms)?;
        txn.commit()
            .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
        return Ok(outcome);
    }

    // 7. Replaceable / addressable — pre-query existing for outcome typing.
    // Returns (replaced_id, replaced_expiry) so the replace path can clean up
    // the expiry index in O(1) without a second event load.
    let pre_existing: Option<(EventId, Option<u64>)> = pre_query_existing(inner, &txn, &event)?;

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
                inner.relay_index,
                inner.relay_kind,
                &mut txn,
                &id_bytes,
                source.clone(),
                event.kind,
                received_at_ms,
            )?;
            // Stamp LRU access for the newly stored event.
            gc::lru_stamp(inner, &mut txn, &id_bytes)?;
            // V-118: index this event's expiry if it carries an expiration tag.
            if let Some(exp) = event.expiration() {
                gc::expiry_index_put(inner, &mut txn, exp, &id_bytes)?;
            }
            if let Some((replaced_id, replaced_expiry)) = pre_existing {
                // Replaced — also drop the replaced event's provenance + LRU entry.
                // A replaceable supersession keeps the same kind, so the replaced
                // event's kind equals the incoming `event.kind`.
                provenance::delete(
                    inner.provenance,
                    inner.relay_index,
                    inner.relay_kind,
                    &mut txn,
                    &replaced_id,
                    event.kind,
                )?;
                gc::lru_delete(inner, &mut txn, &replaced_id)?;
                // V-118: O(1) expiry-index cleanup using the known expiry timestamp.
                gc::expiry_index_delete_exact(inner, &mut txn, replaced_expiry, &replaced_id)?;
                // Delete the replaceable_freshness row so stale TTL data cannot
                // cause a re-fetch to be wrongly skipped after replacement.
                if let Some(freshness_key) = freshness_key_for(&event) {
                    inner
                        .lmdb
                        .delete_freshness(&mut txn, &freshness_key)
                        .map_err(|e| StoreError::Io(format!("delete_freshness: {e}")))?;
                }
                InsertOutcome::Replaced {
                    new_id: id_bytes,
                    replaced_id,
                }
            } else {
                InsertOutcome::Inserted {
                    id: id_bytes,
                    sources_after: count,
                }
            }
        }
        SaveEventStatus::Rejected(RejectedReason::Duplicate) => {
            let count = provenance::upsert(
                inner.provenance,
                inner.relay_index,
                inner.relay_kind,
                &mut txn,
                &id_bytes,
                source.clone(),
                event.kind,
                received_at_ms,
            )?;
            InsertOutcome::Duplicate {
                id: id_bytes,
                sources_after: count,
            }
        }
        SaveEventStatus::Rejected(RejectedReason::Replaced) => {
            // The fork's "Replaced" rejection = incoming is older than what
            // we have — Mem's `Superseded { id, current_id }`. The
            // `current_id` is whatever pre_query found.
            InsertOutcome::Superseded {
                id: id_bytes,
                current_id: pre_existing.map(|(id, _)| id).unwrap_or(id_bytes),
            }
        }
        SaveEventStatus::Rejected(RejectedReason::Deleted) => {
            // Look up tombstone metadata.
            let tomb = tombstones::get(inner.tombstones, &txn, &id_bytes)?;
            let (kind5_event_id, origin) = match tomb {
                Some(t) => (t.kind5_event_id, t.origin),
                None => (None, TombstoneOrigin::AdminPurge),
            };
            InsertOutcome::Tombstoned {
                id: id_bytes,
                kind5_event_id,
                origin,
            }
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

    txn.commit()
        .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    Ok(outcome)
}

/// Build the `ReplaceableKey` for `event` if it is replaceable or
/// param-replaceable.  Returns `None` for regular (non-replaceable) events.
///
/// Used to delete stale `replaceable_freshness` entries on replacement or
/// deletion so a stale TTL never causes a re-fetch to be wrongly skipped.
fn freshness_key_for(event: &RawEvent) -> Option<nmp_nostr_lmdb::ReplaceableKey> {
    let pubkey = event.pubkey_bytes()?;
    let pubkey_arr: [u8; 32] = pubkey.try_into().ok()?;
    let kind = event.kind as u32;
    if event.is_replaceable() {
        Some(nmp_nostr_lmdb::ReplaceableKey::Regular {
            kind,
            pubkey: pubkey_arr,
        })
    } else if event.is_param_replaceable() {
        let d_tag = event.d_tag().map(|d| String::from_utf8_lossy(&d).into_owned()).unwrap_or_default();
        Some(nmp_nostr_lmdb::ReplaceableKey::Parameterized {
            kind,
            pubkey: pubkey_arr,
            d_tag,
        })
    } else {
        None
    }
}

/// Look up the existing event id (and expiry timestamp) for a replaceable /
/// addressable so the outcome can carry `replaced_id` / `current_id` and the
/// replace path can clean up the expiry index in O(1).
///
/// Returns `None` for non-replaceable kinds or when nothing matches.
fn pre_query_existing(
    inner: &Arc<Inner>,
    txn: &heed::RwTxn,
    event: &RawEvent,
) -> Result<Option<(EventId, Option<u64>)>, StoreError> {
    use nostr::prelude::*;
    // pre_query_existing is only called after is_structurally_valid() passes,
    // so pubkey_bytes() is guaranteed Some.
    if event.is_replaceable() {
        let pk_bytes = event
            .pubkey_bytes()
            .expect("passed is_structurally_valid: pubkey is valid hex");
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
                let owned = ev.into_owned();
                let mut id = [0u8; 32];
                id.copy_from_slice(owned.id.as_bytes());
                let expiry = owned.tags.expiration().map(|ts| ts.as_secs());
                Ok(Some((id, expiry)))
            }
            None => Ok(None),
        }
    } else if event.is_param_replaceable() {
        let d = match event.d_tag() {
            Some(d) => d,
            None => return Ok(None),
        };
        let pk_bytes = event
            .pubkey_bytes()
            .expect("passed is_structurally_valid: pubkey is valid hex");
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
                let owned = ev.into_owned();
                let mut id = [0u8; 32];
                id.copy_from_slice(owned.id.as_bytes());
                let expiry = owned.tags.expiration().map(|ts| ts.as_secs());
                Ok(Some((id, expiry)))
            }
            None => Ok(None),
        }
    } else {
        Ok(None)
    }
}

/// Hex-eq for the deleter_pubkey check. `dp` is `[u8; 32]`; `pubkey_hex`
/// is lowercase hex. Returns `false` for non-hex or wrong-length input.
fn hex_eq(dp: &[u8; 32], pubkey_hex: &str) -> bool {
    match RawEvent::hex_to_bytes32_owned(pubkey_hex) {
        Some(parsed) => &parsed == dp,
        None => false,
    }
}

// kind:5 handling lives in `insert_kind5.rs` (LOC-cap split).
// delete_by_filter lives in `delete.rs` (LOC-cap split).
