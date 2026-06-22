//! §7.1 insert invariants for `MemEventStore`.
//!
//! D4: ONE writer. All event mutations flow through here.
//! D2: Returns typed `InsertOutcome`; never panics.
//!
//! P2 fixes applied here:
//!   - Duplicate check BEFORE kind-specific supersession (provenance merge).
//!   - Tombstone max-merge (`deleted_at` max + source union instead of `or_insert`).
//!
//! NIP-09 (kind:5) logic extracted to `insert_kind5.rs` for the 500 LOC cap.

use std::sync::Arc;

use super::fts::{fts_index_add, fts_index_remove};
use super::{access_remove, access_stamp, bytes_to_hex, relay_index_add, relay_index_remove, relay_kind_add, relay_kind_remove_id, upsert_provenance, MemEventStore, MemState};
use super::ic::{ic_decrement, ic_increment};
use super::insert_kind5;
use super::ingest_log;
use crate::ingest_log::DeleteReason;
use crate::types::{
    DeleteFilter, InsertOutcome, RawEvent, RejectReason, RelayUrl, StoredEvent, TombstoneOrigin,
};
use crate::StoreError;
use crate::types::hex_to_event_id;

// ─── Public entry points ─────────────────────────────────────────────────────

pub(super) fn insert(
    store: &MemEventStore,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> Result<InsertOutcome, StoreError> {
    // 1. Structural validation (sig check deferred to nostr crate wiring).
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

    // 2. Ephemeral: deliver to live consumers, do not store.
    if event.is_ephemeral() {
        return Ok(InsertOutcome::Ephemeral {
            id: event.id_bytes().expect("passed is_structurally_valid"),
        });
    }

    // 3. Check NIP-40 expiration on arrival.
    if let Some(exp) = event.expiration() {
        let now_secs = received_at_ms / 1000;
        if exp <= now_secs {
            return Ok(InsertOutcome::Rejected {
                id: event.id_bytes().expect("passed is_structurally_valid"),
                reason: RejectReason::ExpiredOnArrival,
            });
        }
    }

    let id_bytes = event.id_bytes().expect("passed is_structurally_valid");
    let id_hex = event.id.clone();
    let mut st = store.lock()?;

    // 4. Check per-id tombstone.
    // Foreign kind:5 pre-tombstones (deleter != author) must NOT block the event.
    if let Some(tomb) = st.tombstones.get(&id_hex).cloned() {
        let applies = match tomb.origin {
            TombstoneOrigin::Kind5 => tomb
                .deleter_pubkey
                .as_ref()
                .is_some_and(|dp| bytes_to_hex(dp) == event.pubkey),
            TombstoneOrigin::NIP40Expiry | TombstoneOrigin::AdminPurge => true,
        };
        if applies {
            return Ok(InsertOutcome::Tombstoned {
                id: id_bytes,
                kind5_event_id: tomb.kind5_event_id,
                origin: tomb.origin,
            });
        }
        // Foreign pre-tombstone — remove and allow insert (invariant 3).
        st.tombstones.remove(&id_hex);
    }

    // 5. Check address tombstone for parameterized replaceables.
    if event.is_param_replaceable() {
        if let Some(d) = event.d_tag() {
            let addr_key = format!(
                "{}:{}:{}",
                event.kind,
                event.pubkey,
                String::from_utf8_lossy(&d)
            );
            if let Some(tomb) = st.addr_tombstones.get(&addr_key) {
                if tomb.deleted_at >= event.created_at {
                    return Ok(InsertOutcome::Tombstoned {
                        id: id_bytes,
                        kind5_event_id: tomb.kind5_event_id,
                        origin: tomb.origin,
                    });
                }
            }
        }
    }

    // 6. Kind:5 self-delete handling.
    if event.kind == 5 {
        return Ok(insert_kind5::handle_kind5_insert(&mut st, event, source, received_at_ms));
    }

    // 7. Replaceable supersession.
    if event.is_replaceable() {
        let key = (event.pubkey.clone(), event.kind, None::<String>);
        return Ok(handle_supersession(
            &mut st,
            event,
            source,
            received_at_ms,
            key,
        ));
    }

    // 8. Parameterized replaceable.
    if event.is_param_replaceable() {
        let d = event
            .d_tag()
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let key = (event.pubkey.clone(), event.kind, d);
        return Ok(handle_supersession(
            &mut st,
            event,
            source,
            received_at_ms,
            key,
        ));
    }

    // 9. Normal insert / duplicate.
    Ok(handle_normal_insert(&mut st, event, source, received_at_ms))
}

pub(super) fn delete_by_filter(
    store: &MemEventStore,
    filter: DeleteFilter,
) -> Result<usize, StoreError> {
    let mut st = store.lock()?;
    let ids_to_remove: Vec<String> = match &filter {
        DeleteFilter::ByRelayOnly(relay) => st
            .events
            .keys()
            .filter(|id| {
                st.provenance
                    .get(*id)
                    .is_some_and(|p| p.len() == 1 && p[0].relay_url == *relay)
            })
            .cloned()
            .collect(),
        DeleteFilter::ByAuthor(pk) => {
            let pk_hex = bytes_to_hex(pk);
            st.events
                .iter()
                .filter(|(_, ev)| ev.raw.pubkey == pk_hex)
                .map(|(id, _)| id.clone())
                .collect()
        }
        DeleteFilter::ByIds(ids) => ids
            .iter()
            .map(|id| bytes_to_hex(id))
            .filter(|h| st.events.contains_key(h))
            .collect(),
        DeleteFilter::ByKindRange { lo, hi } => st
            .events
            .iter()
            .filter(|(_, ev)| ev.raw.kind >= *lo && ev.raw.kind <= *hi)
            .map(|(id, _)| id.clone())
            .collect(),
    };
    let emit_purge = !matches!(filter, DeleteFilter::ByRelayOnly(_));
    let count = ids_to_remove.len();
    for id in ids_to_remove {
        // Capture kind+tags before removal for counter decrement.
        let ic_data = st.events.get(&id).map(|ev| (ev.raw.kind, ev.raw.tags.clone()));
        st.events.remove(&id);
        st.provenance.remove(&id);
        relay_index_remove(&mut *st, &id);
        relay_kind_remove_id(&mut *st, &id);
        fts_index_remove(&mut *st, &id);
        access_remove(&mut *st, &id);
        // Issue #1519: decrement counter for deleted event.
        if let Some((ik, ref it)) = ic_data {
            ic_decrement(&mut *st, ik, it);
        }
        // ADR-0058 §3: emit AdminPurge log entry for semantic deletions.
        // ByRelayOnly is a retention removal (no log); others are admin purges.
        if emit_purge {
            if let Some(event_id) = hex_to_event_id(&id) {
                ingest_log::emit_deleted(
                    &mut *st,
                    event_id,
                    event_id,
                    DeleteReason::AdminPurge,
                    0,
                );
            }
        }
    }
    Ok(count)
}

// ─── Shared supersession helper ───────────────────────────────────────────────

/// Unified supersession logic for both replaceable and param-replaceable kinds.
/// `key` = (`pubkey_hex`, kind, Option<`d_tag_str`>) — None means any d-tag (replaceable).
fn handle_supersession(
    st: &mut MemState,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
    key: (String, u32, Option<String>),
) -> InsertOutcome {
    let id_bytes = event.id_bytes().expect("passed is_structurally_valid");
    let id_hex = event.id.clone();
    let (pubkey_hex, kind, d_tag_filter) = key;

    // P2 fix: exact-id duplicate BEFORE supersession check.
    if st.events.contains_key(&id_hex) {
        let sources_after = {
            let p = st.provenance.entry(id_hex.clone()).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);
            p.len() as u32
        };
        relay_index_add(st, source, &id_hex);
        relay_kind_add(st, source, kind, &id_hex);
        return InsertOutcome::Duplicate {
            id: id_bytes,
            sources_after,
        };
    }

    let existing_id: Option<String> = st
        .events
        .iter() // O(N) — full scan: no index over (pubkey, kind, d_tag).
        .filter(|(_, ev)| {
            ev.raw.pubkey == pubkey_hex
                && ev.raw.kind == kind
                && match &d_tag_filter {
                    None => true,
                    Some(d) => ev
                        .raw
                        .d_tag()
                        .is_some_and(|tag| String::from_utf8_lossy(&tag).into_owned() == *d),
                }
        })
        .max_by(|(_, a), (_, b)| {
            a.raw
                .created_at
                .cmp(&b.raw.created_at)
                .then(b.raw.id.cmp(&a.raw.id))
        })
        .map(|(id, _)| id.clone());

    if let Some(ref existing_hex) = existing_id {
        let existing_ev = &st.events[existing_hex];
        let existing_time = existing_ev.raw.created_at;
        let existing_id_str = existing_ev.raw.id.clone();
        let incoming_wins = event.created_at > existing_time
            || (event.created_at == existing_time && event.id < existing_id_str);

        if incoming_wins {
            // existing_hex is a key from st.events — it is a stored (verified) event id.
            let replaced_id = RawEvent::hex_to_bytes32_owned(existing_hex)
                .expect("stored event key is valid hex");
            // Capture kind+tags of replaced event BEFORE removal for counter decrement.
            let replaced_ic = st.events.get(existing_hex).map(|ev| {
                (ev.raw.kind, ev.raw.tags.clone())
            });
            st.events.remove(existing_hex);
            st.provenance.remove(existing_hex);
            relay_index_remove(st, existing_hex);
            relay_kind_remove_id(st, existing_hex);
            fts_index_remove(st, existing_hex);
            access_remove(st, existing_hex);
            // Issue #1519: decrement counter for replaced event.
            if let Some((rk, ref rt)) = replaced_ic {
                ic_decrement(st, rk, rt);
            }
            let new_id = id_bytes;
            // Capture kind+tags before moving event into StoredEvent.
            let new_ic_kind = event.kind;
            let new_ic_tags = event.tags.clone();
            // ADR-0058 §3: clone raw event for ingest log before the Arc::new move.
            let raw_for_log = event.clone();
            st.events.insert(
                id_hex.clone(),
                StoredEvent {
                    raw: Arc::new(event),
                    received_at_ms,
                },
            );
            access_stamp(st, &id_hex);
            fts_add_by_id(st, &id_hex);
            // Issue #1519: increment counter for new event.
            ic_increment(st, new_ic_kind, &new_ic_tags);
            let p = st.provenance.entry(id_hex.clone()).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);
            relay_index_add(st, source, &id_hex);
            relay_kind_add(st, source, kind, &id_hex);
            // ADR-0058 §3: emit Replaced log entry.
            ingest_log::emit_replaced(
                st,
                new_id,
                replaced_id,
                raw_for_log,
                source,
                received_at_ms,
            );
            InsertOutcome::Replaced {
                new_id,
                replaced_id,
            }
        } else {
            // existing_hex is a key from st.events — it is a stored (verified) event id.
            InsertOutcome::Superseded {
                id: id_bytes,
                current_id: RawEvent::hex_to_bytes32_owned(existing_hex)
                    .expect("stored event key is valid hex"),
            }
        }
    } else {
        // Capture kind+tags before moving event into StoredEvent.
        let ic_kind = event.kind;
        let ic_tags = event.tags.clone();
        // ADR-0058 §3: clone raw event for ingest log before the Arc::new move.
        let raw_for_log = event.clone();
        st.events.insert(
            id_hex.clone(),
            StoredEvent {
                raw: Arc::new(event),
                received_at_ms,
            },
        );
        access_stamp(st, &id_hex);
        fts_add_by_id(st, &id_hex);
        // Issue #1519: increment interaction counter.
        ic_increment(st, ic_kind, &ic_tags);
        let sources_after = {
            let p = st.provenance.entry(id_hex.clone()).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);
            p.len() as u32
        };
        relay_index_add(st, source, &id_hex);
        relay_kind_add(st, source, kind, &id_hex);
        // ADR-0058 §3: emit Inserted log entry.
        ingest_log::emit_inserted(
            st,
            id_bytes,
            raw_for_log,
            source,
            received_at_ms,
        );
        InsertOutcome::Inserted {
            id: id_bytes,
            sources_after,
        }
    }
}

/// Index the just-inserted event (looked up by hex id) into every installed
/// FTS scope. Re-reads from `st.events` so we don't borrow the moved `event`.
/// Cheap: `StoredEvent` is `Arc<RawEvent>` so the clone is a refcount bump.
fn fts_add_by_id(st: &mut MemState, id_hex: &str) {
    if st.fts.specs.is_empty() {
        return;
    }
    if let Some(stored) = st.events.get(id_hex).cloned() {
        fts_index_add(st, &stored);
    }
}

fn handle_normal_insert(
    st: &mut MemState,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> InsertOutcome {
    let id_bytes = event.id_bytes().expect("passed is_structurally_valid");
    let id_hex = event.id.clone();
    let kind = event.kind;

    if st.events.contains_key(&id_hex) {
        let sources_after = {
            let p = st.provenance.entry(id_hex.clone()).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);
            p.len() as u32
        };
        relay_index_add(st, source, &id_hex);
        relay_kind_add(st, source, kind, &id_hex);
        return InsertOutcome::Duplicate {
            id: id_bytes,
            sources_after,
        };
    }

    // Capture kind+tags before moving event into StoredEvent.
    let ic_kind = event.kind;
    let ic_tags = event.tags.clone();
    // ADR-0058 §3: clone raw event for ingest log before the Arc::new move.
    let raw_for_log = event.clone();
    st.events.insert(
        id_hex.clone(),
        StoredEvent {
            raw: Arc::new(event),
            received_at_ms,
        },
    );
    access_stamp(st, &id_hex);
    fts_add_by_id(st, &id_hex);
    // Issue #1519: increment interaction counter.
    ic_increment(st, ic_kind, &ic_tags);
    let sources_after = {
        let p = st.provenance.entry(id_hex.clone()).or_default();
        upsert_provenance(p, source.clone(), received_at_ms);
        p.len() as u32
    };
    relay_index_add(st, source, &id_hex);
    relay_kind_add(st, source, kind, &id_hex);
    // ADR-0058 §3: emit Inserted log entry.
    ingest_log::emit_inserted(
        st,
        id_bytes,
        raw_for_log,
        source,
        received_at_ms,
    );
    InsertOutcome::Inserted {
        id: id_bytes,
        sources_after,
    }
}

