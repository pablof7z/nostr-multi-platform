//! NIP-09 (kind:5) deletion handler for `MemEventStore`.
//!
//! Extracted from `insert.rs` to keep that file under the 500 LOC hard cap.
//! Entry point is `handle_kind5_insert` — called exclusively by `insert.rs`.
//! D4: all mutations flow through `handle_kind5_insert`; callers must not
//! bypass this path.

use std::collections::HashMap;
use std::sync::Arc;

use super::ic::{ic_decrement, ic_increment};
use super::ingest_log;
use super::{
    access_remove, access_stamp, relay_index_add, relay_index_remove, relay_kind_add,
    relay_kind_remove_id, upsert_provenance, MemState,
};
use crate::ingest_log::DeleteReason;
use crate::types::{InsertOutcome, RawEvent, RelayUrl, StoredEvent, TombstoneOrigin, TombstoneRow};

// ─── Public entry point ───────────────────────────────────────────────────────

pub(super) fn handle_kind5_insert(
    st: &mut MemState,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> InsertOutcome {
    let kind5_id_bytes = event.id_bytes().expect("passed is_structurally_valid");
    let kind5_id_hex = event.id.clone();
    let kind5_pubkey = event.pubkey.clone();
    let kind5_at = event.created_at;

    // BLOCKING 1: Duplicate check — re-delivered kind:5 must not re-apply delete
    // side-effects or emit new log rows. Parity: lmdb/insert_kind5.rs:308-326
    // (`has_event` check before the final `store` call).
    if st.events.contains_key(&kind5_id_hex) {
        let sources_after = {
            let p = st.provenance.entry(kind5_id_hex.clone()).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);
            p.len() as u32
        };
        relay_index_add(st, source, &kind5_id_hex);
        relay_kind_add(st, source, 5, &kind5_id_hex);
        return InsertOutcome::Duplicate {
            id: kind5_id_bytes,
            sources_after,
        };
    }

    // Process `e`-tag deletes (self-deletes only).
    for target_hex in event.e_tags() {
        if let Some(existing) = st.events.get(&target_hex) {
            if existing.raw.pubkey != kind5_pubkey {
                continue;
            }
            // existing.raw is stored (verified) — id_bytes() is guaranteed Some.
            let target_id = existing
                .raw
                .id_bytes()
                .expect("stored event has valid hex id");
            // Capture kind+tags BEFORE removal for counter decrement.
            let ic_kind = existing.raw.kind;
            let ic_tags = existing.raw.tags.clone();
            st.events.remove(&target_hex);
            st.provenance.remove(&target_hex);
            relay_index_remove(st, &target_hex);
            relay_kind_remove_id(st, &target_hex);
            access_remove(st, &target_hex);
            // Issue #1519: decrement counter for removed event.
            ic_decrement(st, ic_kind, &ic_tags);
            // ADR-0058 §3: emit Deleted(Nip09) for each self-deleted target.
            ingest_log::emit_deleted(
                st,
                kind5_id_bytes,
                target_id,
                DeleteReason::Nip09,
                kind5_at * 1000,
            );
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

        // BLOCKING 3: split addressable vs regular-replaceable target matching.
        // Parity by construction: use the SAME nostr predicates LMDB uses
        // (lmdb/insert_kind5.rs:168 `coord.kind.is_addressable()` / :230
        // `coord.kind.is_replaceable()`) instead of hand-rolled ranges — the
        // hand-rolled set silently dropped kind 41 (ChannelMetadata), which
        // `Kind::is_replaceable()` includes, so a kind:5 a-tag targeting a
        // kind:41 event removed/logged on LMDB but not Mem. Single source of
        // truth for the kind classification removes that class of divergence.
        // A kind that is neither addressable nor replaceable cannot be the
        // target of an a-tag coordinate — skip deletion entirely.
        let tgt_kind_obj = nostr::Kind::from(tgt_kind as u16);
        let is_addressable_kind = tgt_kind_obj.is_addressable();
        let is_replaceable_kind = tgt_kind_obj.is_replaceable();

        let to_delete: Vec<String> = st
            .events
            .iter()
            .filter(|(_, ev)| {
                ev.raw.pubkey == tgt_pk
                    && ev.raw.kind == tgt_kind
                    && ev.raw.created_at <= kind5_at
                    && if is_addressable_kind {
                        // Addressable: must match the d-tag from the a-tag coord.
                        ev.raw
                            .d_tag()
                            .is_some_and(|d| String::from_utf8_lossy(&d).into_owned() == tgt_dtag)
                    } else if is_replaceable_kind {
                        // Regular replaceable: kind+pubkey only — no d-tag required.
                        true
                    } else {
                        // Neither addressable nor replaceable — a-tag cannot target it.
                        false
                    }
            })
            .map(|(id, _)| id.clone())
            .collect();

        for target_hex in to_delete {
            // Capture kind+tags BEFORE removal for counter decrement.
            let ic_data = st
                .events
                .get(&target_hex)
                .map(|ev| (ev.raw.kind, ev.raw.tags.clone()));
            if let Some(existing) = st.events.remove(&target_hex) {
                st.provenance.remove(&target_hex);
                relay_index_remove(st, &target_hex);
                relay_kind_remove_id(st, &target_hex);
                access_remove(st, &target_hex);
                // Issue #1519: decrement counter for removed event.
                if let Some((ik, ref it)) = ic_data {
                    ic_decrement(st, ik, it);
                }
                // existing.raw is stored (verified) — id_bytes() is guaranteed Some.
                let target_id = existing
                    .raw
                    .id_bytes()
                    .expect("stored event has valid hex id");
                // ADR-0058 §3: emit Deleted(Nip09) for each self-deleted a-tag target.
                ingest_log::emit_deleted(
                    st,
                    kind5_id_bytes,
                    target_id,
                    DeleteReason::Nip09,
                    kind5_at * 1000,
                );
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
    // Capture kind+tags before move (kind:5 is not a counter kind — no-op, but uniform).
    let k5_ic_kind = event.kind;
    let k5_ic_tags = event.tags.clone();
    // ADR-0058 §3: clone raw event for ingest log before the Arc::new move.
    let raw_for_log = event.clone();
    st.events.insert(
        kind5_id_hex.clone(),
        StoredEvent {
            raw: Arc::new(event),
            received_at_ms,
        },
    );
    access_stamp(st, &kind5_id_hex);
    // Issue #1519: increment counter (no-op for kind:5).
    ic_increment(st, k5_ic_kind, &k5_ic_tags);
    let sources_after = {
        let p = st.provenance.entry(kind5_id_hex.clone()).or_default();
        upsert_provenance(p, source.clone(), received_at_ms);
        p.len() as u32
    };
    relay_index_add(st, source, &kind5_id_hex);
    relay_kind_add(st, source, 5, &kind5_id_hex);
    // ADR-0058 §3: emit Inserted log entry for the kind:5 event itself.
    ingest_log::emit_inserted(st, kind5_id_bytes, raw_for_log, source, received_at_ms);
    InsertOutcome::Inserted {
        id: kind5_id_bytes,
        sources_after,
    }
}

// ─── Tombstone helpers ────────────────────────────────────────────────────────

pub(super) fn kind5_tomb(
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
pub(super) fn merge_tombstone(
    map: &mut HashMap<String, TombstoneRow>,
    key: String,
    incoming: TombstoneRow,
) {
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
