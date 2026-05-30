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

use super::{gc, provenance, Inner};
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
        if inner
            .lmdb
            .has_event(txn, &id)
            .map_err(|e| StoreError::Io(format!("has: {e}")))?
        {
            let f = Filter::new().id(nostr::EventId::from_slice(&id)
                .map_err(|e| StoreError::Encoding(format!("id: {e}")))?);
            inner
                .lmdb
                .delete(txn, f)
                .map_err(|e| StoreError::Io(format!("del: {e}")))?;
            provenance::delete(inner.provenance, txn, &id)?;
            gc::lru_delete(inner, txn, &id)?;
            n += 1;
        }
    }
    Ok(n)
}

fn by_author(inner: &Arc<Inner>, txn: &mut heed::RwTxn, pk: EventId) -> Result<usize, StoreError> {
    let pk = PublicKey::from_slice(&pk).map_err(|e| StoreError::Encoding(format!("pk: {e}")))?;
    let f = Filter::new().author(pk);
    let ids: Vec<EventId> = inner
        .lmdb
        .query(txn, f.clone())
        .map_err(|e| StoreError::Io(format!("q: {e}")))?
        .map(|ev| {
            let mut id = [0u8; 32];
            id.copy_from_slice(ev.id);
            id
        })
        .collect();
    let n = ids.len();
    inner
        .lmdb
        .delete(txn, f)
        .map_err(|e| StoreError::Io(format!("del: {e}")))?;
    for id in ids {
        provenance::delete(inner.provenance, txn, &id)?;
        gc::lru_delete(inner, txn, &id)?;
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
    let ids: Vec<EventId> = inner
        .lmdb
        .query(txn, f.clone())
        .map_err(|e| StoreError::Io(format!("q: {e}")))?
        .map(|ev| {
            let mut id = [0u8; 32];
            id.copy_from_slice(ev.id);
            id
        })
        .collect();
    let n = ids.len();
    inner
        .lmdb
        .delete(txn, f)
        .map_err(|e| StoreError::Io(format!("del: {e}")))?;
    for id in ids {
        provenance::delete(inner.provenance, txn, &id)?;
        gc::lru_delete(inner, txn, &id)?;
    }
    Ok(n)
}

fn by_relay_only(
    inner: &Arc<Inner>,
    txn: &mut heed::RwTxn,
    relay: String,
) -> Result<usize, StoreError> {
    let mut victims: Vec<EventId> = Vec::new();
    for entry in inner
        .provenance
        .iter(txn)
        .map_err(|e| StoreError::Io(format!("prov iter: {e}")))?
    {
        let (k, v) = entry.map_err(|e| StoreError::Io(format!("prov step: {e}")))?;
        if k.len() != 32 {
            continue;
        }
        let entries = decode_local(v)?;
        if entries.len() == 1 && entries[0].relay_url == relay {
            let mut id = [0u8; 32];
            id.copy_from_slice(k);
            victims.push(id);
        }
    }
    let n = victims.len();
    for id in victims {
        let f = Filter::new().id(nostr::EventId::from_slice(&id)
            .map_err(|e| StoreError::Encoding(format!("id: {e}")))?);
        inner
            .lmdb
            .delete(txn, f)
            .map_err(|e| StoreError::Io(format!("del: {e}")))?;
        provenance::delete(inner.provenance, txn, &id)?;
        gc::lru_delete(inner, txn, &id)?;
    }
    Ok(n)
}
