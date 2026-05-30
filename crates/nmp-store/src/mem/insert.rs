//! §7.1 insert invariants for `MemEventStore`.
//!
//! D4: ONE writer. All event mutations flow through here.
//! D2: Returns typed `InsertOutcome`; never panics.
//!
//! P2 fixes applied here:
//!   - Duplicate check BEFORE kind-specific supersession (provenance merge).
//!   - Tombstone max-merge (`deleted_at` max + source union instead of `or_insert`).

use std::collections::HashMap;
use std::sync::Arc;

use super::{access_remove, access_stamp, bytes_to_hex, relay_index_add, relay_index_remove, upsert_provenance, MemEventStore, MemState};
use crate::types::{
    DeleteFilter, InsertOutcome, RawEvent, RejectReason, RelayUrl, StoredEvent, TombstoneOrigin,
    TombstoneRow,
};
use crate::StoreError;

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
        return Ok(handle_kind5_insert(&mut st, event, source, received_at_ms));
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
    let count = ids_to_remove.len();
    for id in ids_to_remove {
        st.events.remove(&id);
        st.provenance.remove(&id);
        relay_index_remove(&mut *st, &id);
        access_remove(&mut *st, &id);
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
            st.events.remove(existing_hex);
            st.provenance.remove(existing_hex);
            relay_index_remove(st, existing_hex);
            access_remove(st, existing_hex);
            let new_id = id_bytes;
            st.events.insert(
                id_hex.clone(),
                StoredEvent {
                    raw: Arc::new(event),
                    received_at_ms,
                },
            );
            access_stamp(st, &id_hex);
            let p = st.provenance.entry(id_hex.clone()).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);
            relay_index_add(st, source, &id_hex);
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
        st.events.insert(
            id_hex.clone(),
            StoredEvent {
                raw: Arc::new(event),
                received_at_ms,
            },
        );
        access_stamp(st, &id_hex);
        let sources_after = {
            let p = st.provenance.entry(id_hex.clone()).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);
            p.len() as u32
        };
        relay_index_add(st, source, &id_hex);
        InsertOutcome::Inserted {
            id: id_bytes,
            sources_after,
        }
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

    if st.events.contains_key(&id_hex) {
        let sources_after = {
            let p = st.provenance.entry(id_hex.clone()).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);
            p.len() as u32
        };
        relay_index_add(st, source, &id_hex);
        return InsertOutcome::Duplicate {
            id: id_bytes,
            sources_after,
        };
    }

    st.events.insert(
        id_hex.clone(),
        StoredEvent {
            raw: Arc::new(event),
            received_at_ms,
        },
    );
    access_stamp(st, &id_hex);
    let sources_after = {
        let p = st.provenance.entry(id_hex.clone()).or_default();
        upsert_provenance(p, source.clone(), received_at_ms);
        p.len() as u32
    };
    relay_index_add(st, source, &id_hex);
    InsertOutcome::Inserted {
        id: id_bytes,
        sources_after,
    }
}

fn handle_kind5_insert(
    st: &mut MemState,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> InsertOutcome {
    let kind5_id_bytes = event.id_bytes().expect("passed is_structurally_valid");
    let kind5_id_hex = event.id.clone();
    let kind5_pubkey = event.pubkey.clone();
    let kind5_at = event.created_at;

    // Process `e`-tag deletes (self-deletes only).
    for target_hex in event.e_tags() {
        if let Some(existing) = st.events.get(&target_hex) {
            if existing.raw.pubkey != kind5_pubkey {
                continue;
            }
            // existing.raw is stored (verified) — id_bytes() is guaranteed Some.
            let target_id = existing.raw.id_bytes().expect("stored event has valid hex id");
            st.events.remove(&target_hex);
            st.provenance.remove(&target_hex);
            relay_index_remove(st, &target_hex);
            access_remove(st, &target_hex);
            merge_tombstone(
                &mut st.tombstones,
                target_hex,
                kind5_tomb(target_id, kind5_id_bytes, &kind5_pubkey, kind5_at, source),
            );
        } else {
            // target_hex is from an e-tag value — may be malformed. Skip if undecidable.
            let Some(target_id) = RawEvent::hex_to_bytes32_owned(&target_hex) else {
                continue;
            };
            merge_tombstone(
                &mut st.tombstones,
                target_hex,
                kind5_tomb(target_id, kind5_id_bytes, &kind5_pubkey, kind5_at, source),
            );
        }
    }

    // Process `a`-tag deletes (parameterized replaceables, self-delete only).
    for addr in event.a_tags() {
        let parts: Vec<&str> = addr.splitn(3, ':').collect();
        if parts.len() < 3 {
            continue;
        }
        let (tgt_kind_str, tgt_pk, tgt_dtag) = (parts[0], parts[1], parts[2]);
        if tgt_pk != kind5_pubkey {
            continue;
        }
        let Ok(tgt_kind) = tgt_kind_str.parse::<u32>() else {
            continue;
        };
        let addr_key = format!("{tgt_kind_str}:{tgt_pk}:{tgt_dtag}");

        let to_delete: Vec<String> = st
            .events
            .iter()
            .filter(|(_, ev)| {
                ev.raw.pubkey == tgt_pk
                    && ev.raw.kind == tgt_kind
                    && ev
                        .raw
                        .d_tag()
                        .is_some_and(|d| String::from_utf8_lossy(&d).into_owned() == tgt_dtag)
                    && ev.raw.created_at <= kind5_at
            })
            .map(|(id, _)| id.clone())
            .collect();

        for target_hex in to_delete {
            if let Some(existing) = st.events.remove(&target_hex) {
                st.provenance.remove(&target_hex);
                relay_index_remove(st, &target_hex);
                access_remove(st, &target_hex);
                // existing.raw is stored (verified) — id_bytes() is guaranteed Some.
                let target_id = existing
                    .raw
                    .id_bytes()
                    .expect("stored event has valid hex id");
                merge_tombstone(
                    &mut st.tombstones,
                    target_hex,
                    kind5_tomb(target_id, kind5_id_bytes, &kind5_pubkey, kind5_at, source),
                );
            }
        }
        // Address tombstone for events arriving later (max-merge).
        // [0u8;32] is a sentinel for "no primary id" on address-tombstones (documented).
        merge_tombstone(
            &mut st.addr_tombstones,
            addr_key,
            kind5_tomb([0u8; 32], kind5_id_bytes, &kind5_pubkey, kind5_at, source),
        );
    }

    // Store the kind:5 event itself.
    st.events.insert(
        kind5_id_hex.clone(),
        StoredEvent {
            raw: Arc::new(event),
            received_at_ms,
        },
    );
    access_stamp(st, &kind5_id_hex);
    let sources_after = {
        let p = st.provenance.entry(kind5_id_hex.clone()).or_default();
        upsert_provenance(p, source.clone(), received_at_ms);
        p.len() as u32
    };
    relay_index_add(st, source, &kind5_id_hex);
    InsertOutcome::Inserted {
        id: kind5_id_bytes,
        sources_after,
    }
}

// ─── Tombstone helpers ────────────────────────────────────────────────────────

fn kind5_tomb(
    target_id: [u8; 32],
    kind5_id: [u8; 32],
    kind5_pubkey: &str,
    deleted_at: u64,
    source: &RelayUrl,
) -> TombstoneRow {
    TombstoneRow {
        target_id,
        kind5_event_id: Some(kind5_id),
        // kind5_pubkey is from a verified event — hex is guaranteed valid.
        deleter_pubkey: Some(
            RawEvent::hex_to_bytes32_owned(kind5_pubkey)
                .expect("kind5 event passed is_structurally_valid: pubkey is valid hex"),
        ),
        deleted_at,
        sources: vec![source.clone()],
        origin: TombstoneOrigin::Kind5,
    }
}

/// P2 fix: tombstone upsert max-merges `deleted_at` and unions sources.
/// Original `or_insert` kept first-arrived timestamp — wrong for re-deliveries.
fn merge_tombstone(map: &mut HashMap<String, TombstoneRow>, key: String, incoming: TombstoneRow) {
    match map.get_mut(&key) {
        Some(existing) => {
            if incoming.deleted_at > existing.deleted_at {
                existing.deleted_at = incoming.deleted_at;
                existing.kind5_event_id = incoming.kind5_event_id;
            }
            for src in incoming.sources {
                if !existing.sources.contains(&src) {
                    existing.sources.push(src);
                }
            }
        }
        None => {
            map.insert(key, incoming);
        }
    }
}

