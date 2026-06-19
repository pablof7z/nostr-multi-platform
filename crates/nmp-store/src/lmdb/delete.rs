//! `delete_by_filter` — admin-only bulk-delete path.
//!
//! Mirrors `mem/insert.rs::delete_by_filter` semantics: the four
//! `DeleteFilter` variants map onto `nostr::Filter` (for the fork's
//! `delete` primitive) plus provenance-cleanup.
//!
//! NOTE: this is not a NIP-09 deletion path — that flows through
//! `kind:5` `insert` calls in `insert.rs`. This method is for GC / admin
//! purge / kind:5-application paths only (D6).

use std::sync::Arc;

use nostr::prelude::*;

use super::{gc, ingest_log, provenance, Inner};
use crate::ingest_log::DeleteReason;
use crate::types::{DeleteFilter, EventId};
use crate::StoreError;

#[derive(serde::Deserialize)]
struct LocalProvenanceEntry {
    relay_url: String,
}

fn decode_local(bytes: &[u8]) -> Result<Vec<LocalProvenanceEntry>, StoreError> {
    serde_json::from_slice(bytes).map_err(|e| StoreError::Encoding(format!("prov decode: {e}")))
}

pub(super) fn delete_by_filter(
    inner: &Arc<Inner>,
    filter: DeleteFilter,
) -> Result<usize, StoreError> {
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;

    let count = match filter {
        DeleteFilter::ByIds(ids) => by_ids(inner, &mut txn, ids)?,
        DeleteFilter::ByAuthor(pk) => by_author(inner, &mut txn, pk)?,
        DeleteFilter::ByKindRange { lo, hi } => by_kind_range(inner, &mut txn, lo, hi)?,
        DeleteFilter::ByRelayOnly(relay) => by_relay_only(inner, &mut txn, relay)?,
    };

    txn.commit()
        .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    Ok(count)
}

fn by_ids(
    inner: &Arc<Inner>,
    txn: &mut heed::RwTxn,
    ids: Vec<EventId>,
) -> Result<usize, StoreError> {
    let mut n = 0usize;
    for id in ids {
        // Load the event to capture its expiry timestamp + kind+tags before deletion.
        let (expiry, kind, event_tags) = match inner
            .lmdb
            .get_event_by_id(txn, &id)
            .map_err(|e| StoreError::Io(format!("get: {e}")))?
        {
            None => continue, // Not stored — skip.
            Some(ev) => {
                let owned = ev.into_owned();
                let expiry = owned.tags.expiration().map(|ts| ts.as_secs());
                let kind = owned.kind.as_u16() as u32;
                let tags: Vec<Vec<String>> = owned.tags.iter().map(|t| t.clone().to_vec()).collect();
                (expiry, kind, Some(tags))
            }
        };
        let f = Filter::new().id(nostr::EventId::from_slice(&id)
            .map_err(|e| StoreError::Encoding(format!("id: {e}")))?);
        inner
            .lmdb
            .delete(txn, f)
            .map_err(|e| StoreError::Io(format!("del: {e}")))?;
        provenance::delete(inner.provenance, inner.relay_index, inner.relay_kind, txn, &id, kind)?;
        gc::lru_delete(inner, txn, &id)?;
        gc::expiry_index_delete_exact(inner, txn, expiry, &id)?;
        // Issue #1519: decrement interaction-counter for deleted event.
        if let Some(ref tags) = event_tags {
            super::interaction_counters::apply_on_remove(
                inner.interaction_counters,
                txn,
                kind,
                tags,
            )?;
        }
        // ADR-0058 §3: emit AdminPurge log entry for semantic deletion.
        ingest_log::append_deleted(
            inner.ingest_log,
            inner.ingest_meta,
            txn,
            &id,
            id,
            DeleteReason::AdminPurge,
            0,
            inner.map_size,
            inner.max_readers,
        )?;
        n += 1;
    }
    Ok(n)
}

fn by_author(inner: &Arc<Inner>, txn: &mut heed::RwTxn, pk: EventId) -> Result<usize, StoreError> {
    let pk = PublicKey::from_slice(&pk).map_err(|e| StoreError::Encoding(format!("pk: {e}")))?;
    let f = Filter::new().author(pk);
    // Collect (id, expiry, kind, tags) before the bulk delete so index cleanup is O(1) per event.
    let victims: Vec<(EventId, Option<u64>, u32, Option<Vec<Vec<String>>>)> = inner
        .lmdb
        .query(txn, f.clone())
        .map_err(|e| StoreError::Io(format!("q: {e}")))?
        .map(|ev| {
            let owned = ev.into_owned();
            let mut id = [0u8; 32];
            id.copy_from_slice(owned.id.as_bytes());
            let expiry = owned.tags.expiration().map(|ts| ts.as_secs());
            let kind = owned.kind.as_u16() as u32;
            let tags: Vec<Vec<String>> = owned.tags.iter().map(|t| t.clone().to_vec()).collect();
            (id, expiry, kind, Some(tags))
        })
        .collect();
    let n = victims.len();
    inner
        .lmdb
        .delete(txn, f)
        .map_err(|e| StoreError::Io(format!("del: {e}")))?;
    for (id, expiry, kind, event_tags) in victims {
        provenance::delete(inner.provenance, inner.relay_index, inner.relay_kind, txn, &id, kind)?;
        gc::lru_delete(inner, txn, &id)?;
        gc::expiry_index_delete_exact(inner, txn, expiry, &id)?;
        // Issue #1519: decrement interaction-counter for deleted event.
        if let Some(ref tags) = event_tags {
            super::interaction_counters::apply_on_remove(
                inner.interaction_counters,
                txn,
                kind,
                tags,
            )?;
        }
        // ADR-0058 §3: emit AdminPurge log entry.
        ingest_log::append_deleted(
            inner.ingest_log,
            inner.ingest_meta,
            txn,
            &id,
            id,
            DeleteReason::AdminPurge,
            0,
            inner.map_size,
            inner.max_readers,
        )?;
    }
    Ok(n)
}

fn by_kind_range(
    inner: &Arc<Inner>,
    txn: &mut heed::RwTxn,
    lo: u32,
    hi: u32,
) -> Result<usize, StoreError> {
    let kinds: Vec<Kind> = (lo..=hi).map(|k| Kind::from(k as u16)).collect();
    let f = Filter::new().kinds(kinds);
    // Collect (id, expiry, kind, tags) before the bulk delete so index cleanup is O(1) per event.
    let victims: Vec<(EventId, Option<u64>, u32, Option<Vec<Vec<String>>>)> = inner
        .lmdb
        .query(txn, f.clone())
        .map_err(|e| StoreError::Io(format!("q: {e}")))?
        .map(|ev| {
            let owned = ev.into_owned();
            let mut id = [0u8; 32];
            id.copy_from_slice(owned.id.as_bytes());
            let expiry = owned.tags.expiration().map(|ts| ts.as_secs());
            let kind = owned.kind.as_u16() as u32;
            let tags: Vec<Vec<String>> = owned.tags.iter().map(|t| t.clone().to_vec()).collect();
            (id, expiry, kind, Some(tags))
        })
        .collect();
    let n = victims.len();
    inner
        .lmdb
        .delete(txn, f)
        .map_err(|e| StoreError::Io(format!("del: {e}")))?;
    for (id, expiry, kind, event_tags) in victims {
        provenance::delete(inner.provenance, inner.relay_index, inner.relay_kind, txn, &id, kind)?;
        gc::lru_delete(inner, txn, &id)?;
        gc::expiry_index_delete_exact(inner, txn, expiry, &id)?;
        // Issue #1519: decrement interaction-counter for deleted event.
        if let Some(ref tags) = event_tags {
            super::interaction_counters::apply_on_remove(
                inner.interaction_counters,
                txn,
                kind,
                tags,
            )?;
        }
        // ADR-0058 §3: emit AdminPurge log entry.
        ingest_log::append_deleted(
            inner.ingest_log,
            inner.ingest_meta,
            txn,
            &id,
            id,
            DeleteReason::AdminPurge,
            0,
            inner.map_size,
            inner.max_readers,
        )?;
    }
    Ok(n)
}

fn by_relay_only(
    inner: &Arc<Inner>,
    txn: &mut heed::RwTxn,
    relay: String,
) -> Result<usize, StoreError> {
    // V-52: candidate ids come from the relay-origin reverse index — an
    // O(events-on-relay) prefix scan — instead of an O(store) provenance scan.
    // Each candidate's provenance is still loaded so only events seen on EXACTLY
    // this relay (`len() == 1`) are deleted; an event also seen on another relay
    // must survive.
    let mut victims: Vec<EventId> = Vec::new();
    {
        let (lo, hi) = provenance::relay_index_prefix_bounds(&relay);
        let range = (
            std::ops::Bound::Included(lo.as_slice()),
            std::ops::Bound::Excluded(hi.as_slice()),
        );
        // Collect candidate ids first so the immutable index borrow ends before
        // the per-candidate provenance reads below.
        let candidates: Vec<EventId> = {
            let mut out = Vec::new();
            for entry in inner
                .relay_index
                .range(txn, &range)
                .map_err(|e| StoreError::Io(format!("relay_index range: {e}")))?
            {
                let (k, _) =
                    entry.map_err(|e| StoreError::Io(format!("relay_index step: {e}")))?;
                if let Some(id) = provenance::relay_index_id_from_key(k, relay.len()) {
                    out.push(id);
                }
            }
            out
        };
        for id in candidates {
            let entries = match inner
                .provenance
                .get(txn, &id)
                .map_err(|e| StoreError::Io(format!("prov get: {e}")))?
            {
                Some(v) => decode_local(v)?,
                None => continue,
            };
            if entries.len() == 1 && entries[0].relay_url == relay {
                victims.push(id);
            }
        }
    }
    let n = victims.len();
    for id in victims {
        // Load expiry + kind+tags before deletion so index cleanup can be O(1).
        let (expiry, kind, event_tags) = match inner
            .lmdb
            .get_event_by_id(txn, &id)
            .map_err(|e| StoreError::Io(format!("get: {e}")))?
        {
            None => (None, 0, None),
            Some(ev) => {
                let owned = ev.into_owned();
                let expiry = owned.tags.expiration().map(|ts| ts.as_secs());
                let kind = owned.kind.as_u16() as u32;
                let tags: Vec<Vec<String>> = owned.tags.iter().map(|t| t.clone().to_vec()).collect();
                (expiry, kind, Some(tags))
            }
        };
        let f = Filter::new().id(nostr::EventId::from_slice(&id)
            .map_err(|e| StoreError::Encoding(format!("id: {e}")))?);
        inner
            .lmdb
            .delete(txn, f)
            .map_err(|e| StoreError::Io(format!("del: {e}")))?;
        provenance::delete(inner.provenance, inner.relay_index, inner.relay_kind, txn, &id, kind)?;
        gc::lru_delete(inner, txn, &id)?;
        gc::expiry_index_delete_exact(inner, txn, expiry, &id)?;
        // Issue #1519: decrement interaction-counter for deleted event.
        if let Some(ref tags) = event_tags {
            super::interaction_counters::apply_on_remove(
                inner.interaction_counters,
                txn,
                kind,
                tags,
            )?;
        }
    }
    Ok(n)
}
