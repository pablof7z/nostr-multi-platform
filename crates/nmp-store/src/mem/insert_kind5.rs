//! NIP-09 (kind:5) deletion handler for `MemEventStore`.
//!
//! Extracted from `insert.rs` to keep that file under the 500 LOC hard cap.
//! Entry point is `handle_kind5_insert` — called exclusively by `insert.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use super::{
    access_remove, access_stamp, relay_index_add, relay_index_remove, relay_kind_add,
    relay_kind_remove_id, upsert_provenance, MemState,
};
use crate::types::{InsertOutcome, RawEvent, RelayUrl, StoredEvent, TombstoneOrigin, TombstoneRow};

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
            relay_kind_remove_id(st, &target_hex);
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
                relay_kind_remove_id(st, &target_hex);
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
    relay_kind_add(st, source, 5, &kind5_id_hex);
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
