//! Read / scan / query_visit methods for the LMDB backend.
//!
//! Strategy: build a `nostr::Filter` from each `EventStore` query method
//! (since/until/kinds/authors/tags/ids), call `Lmdb::query`, then convert
//! each returned `EventBorrow` back to `RawEvent`/`StoredEvent`. The fork's
//! `BTreeSet`-backed `query` already produces newest-first ordering by
//! `(created_at desc, id desc)`; the Mem invariant is `(created_at desc,
//! id asc)`. We post-sort the materialized vec to match Mem's order.

use std::ops::ControlFlow;
use std::sync::Arc;

use nostr::prelude::*;

use super::{conv, provenance, tombstones, Inner};
use crate::store::events::EventIter;
use crate::store::types::{
    Coverage, EventId, ProvenanceEntry, PubKey, RelayUrl, StoreQuery, StoredEvent, SyncMethod,
    TombstoneRow, WatermarkKey, WatermarkRow, COVERAGE_STALENESS_WINDOW_SECS,
};
use crate::store::StoreError;

// ─── Primary lookup ──────────────────────────────────────────────────────────

pub(super) fn get_by_id(
    inner: &Arc<Inner>,
    id: &EventId,
) -> Result<Option<StoredEvent>, StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    // Tombstoned events MUST NOT be returned by get_by_id.
    if tombstones::get(inner.tombstones, &txn, id)?.is_some() {
        return Ok(None);
    }
    let Some(borrow) = inner
        .lmdb
        .get_event_by_id(&txn, id)
        .map_err(|e| StoreError::Io(format!("get: {e}")))?
    else {
        return Ok(None);
    };
    let owned: Event = borrow.into_owned();
    let raw = conv::nostr_to_raw(&owned)?;
    Ok(Some(conv::stored_from_raw(raw, /* received_at_ms */ 0)))
}

// ─── Scans ───────────────────────────────────────────────────────────────────

fn run_filter(
    inner: &Arc<Inner>,
    filter: Filter,
    limit: usize,
) -> Result<Vec<StoredEvent>, StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let filter = filter.limit(limit);
    let iter = inner
        .lmdb
        .query(&txn, filter)
        .map_err(|e| StoreError::Io(format!("query: {e}")))?;
    let mut out: Vec<StoredEvent> = Vec::with_capacity(limit.min(64));
    for ev in iter {
        let owned: Event = ev.into_owned();
        let raw = conv::nostr_to_raw(&owned)?;
        out.push(conv::stored_from_raw(raw, 0));
    }
    // Mem orders newest-first by (created_at desc, id asc). Match it.
    out.sort_by(|a, b| {
        b.raw.created_at
            .cmp(&a.raw.created_at)
            .then(a.raw.id.cmp(&b.raw.id))
    });
    out.truncate(limit);
    Ok(out)
}

pub(super) fn scan_by_author_kind<'a>(
    inner: &'a Arc<Inner>,
    author: &PubKey,
    kinds: &[u32],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let pk = PublicKey::from_slice(author)
        .map_err(|e| StoreError::Encoding(format!("pk: {e}")))?;
    let mut f = Filter::new().author(pk);
    if !kinds.is_empty() {
        f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
    }
    if let Some(s) = since {
        f = f.since(Timestamp::from_secs(s));
    }
    if let Some(u) = until {
        f = f.until(Timestamp::from_secs(u));
    }
    let v = run_filter(inner, f, limit)?;
    Ok(Box::new(v.into_iter().map(Ok)))
}

pub(super) fn scan_by_kind_time<'a>(
    inner: &'a Arc<Inner>,
    kinds: &[u32],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let mut f = Filter::new();
    if !kinds.is_empty() {
        f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
    }
    if let Some(s) = since {
        f = f.since(Timestamp::from_secs(s));
    }
    if let Some(u) = until {
        f = f.until(Timestamp::from_secs(u));
    }
    let v = run_filter(inner, f, limit)?;
    Ok(Box::new(v.into_iter().map(Ok)))
}

pub(super) fn scan_by_kind_dtag<'a>(
    inner: &'a Arc<Inner>,
    kind: u32,
    d_tag: &[u8],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let d_str = String::from_utf8_lossy(d_tag).into_owned();
    let mut f = Filter::new()
        .kind(Kind::from(kind as u16))
        .identifier(d_str);
    if let Some(s) = since {
        f = f.since(Timestamp::from_secs(s));
    }
    if let Some(u) = until {
        f = f.until(Timestamp::from_secs(u));
    }
    let v = run_filter(inner, f, limit)?;
    Ok(Box::new(v.into_iter().map(Ok)))
}

pub(super) fn scan_by_etag<'a>(
    inner: &'a Arc<Inner>,
    target: &EventId,
    kinds: &[u32],
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let target = nostr::EventId::from_slice(target)
        .map_err(|e| StoreError::Encoding(format!("id: {e}")))?;
    let mut f = Filter::new().event(target);
    if !kinds.is_empty() {
        f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
    }
    let v = run_filter(inner, f, limit)?;
    Ok(Box::new(v.into_iter().map(Ok)))
}

pub(super) fn scan_by_ptag<'a>(
    inner: &'a Arc<Inner>,
    target: &PubKey,
    kinds: &[u32],
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let pk = PublicKey::from_slice(target)
        .map_err(|e| StoreError::Encoding(format!("pk: {e}")))?;
    let mut f = Filter::new().pubkey(pk);
    if !kinds.is_empty() {
        f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
    }
    let v = run_filter(inner, f, limit)?;
    Ok(Box::new(v.into_iter().map(Ok)))
}

pub(super) fn get_param_replaceable(
    inner: &Arc<Inner>,
    pubkey: &PubKey,
    kind: u32,
    d_tag: &[u8],
) -> Result<Option<StoredEvent>, StoreError> {
    let pk = PublicKey::from_slice(pubkey)
        .map_err(|e| StoreError::Encoding(format!("pk: {e}")))?;
    let d_str = String::from_utf8_lossy(d_tag).into_owned();
    let coord = Coordinate::new(Kind::from(kind as u16), pk).identifier(d_str);
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let Some(borrow) = inner
        .lmdb
        .find_addressable_event(&txn, &coord)
        .map_err(|e| StoreError::Io(format!("find_addr: {e}")))?
    else {
        return Ok(None);
    };
    let owned: Event = borrow.into_owned();
    let raw = conv::nostr_to_raw(&owned)?;
    Ok(Some(conv::stored_from_raw(raw, 0)))
}

pub(super) fn scan_expiring_before<'a>(
    inner: &'a Arc<Inner>,
    unix_seconds: u64,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    // Mem scans every stored event for an `expiration` tag < unix_seconds,
    // ascending by expiration. The fork has no expiration index — emulate by
    // scanning the full ci_index. This is O(N) like Mem; acceptable for the
    // GC reaper path.
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let filter = Filter::new();
    let iter = inner
        .lmdb
        .query(&txn, filter)
        .map_err(|e| StoreError::Io(format!("query: {e}")))?;
    let mut pairs: Vec<(u64, StoredEvent)> = Vec::new();
    for ev in iter {
        let owned: Event = ev.into_owned();
        let raw = conv::nostr_to_raw(&owned)?;
        if let Some(exp) = raw.expiration() {
            if exp < unix_seconds {
                pairs.push((exp, conv::stored_from_raw(raw, 0)));
            }
        }
    }
    pairs.sort_by_key(|(exp, _)| *exp);
    pairs.truncate(limit);
    Ok(Box::new(pairs.into_iter().map(|(_, ev)| Ok(ev))))
}

// ─── query_visit ─────────────────────────────────────────────────────────────

pub(super) fn query_visit(
    inner: &Arc<Inner>,
    query: &StoreQuery,
    limit: usize,
    visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
) -> Result<(), StoreError> {
    if limit == 0 {
        return Ok(());
    }
    let matched = match query {
        StoreQuery::AuthorKind { author, kinds, since, until } => {
            let v = collect(scan_by_author_kind(inner, author, kinds, *since, *until, limit)?)?;
            v
        }
        StoreQuery::KindTime { kinds, since, until } => {
            collect(scan_by_kind_time(inner, kinds, *since, *until, limit)?)?
        }
        StoreQuery::KindDtag { kind, d_tag, since, until } => {
            collect(scan_by_kind_dtag(inner, *kind, d_tag, *since, *until, limit)?)?
        }
        StoreQuery::Etag { target, kinds } => {
            collect(scan_by_etag(inner, target, kinds, limit)?)?
        }
        StoreQuery::Ptag { target, kinds } => {
            collect(scan_by_ptag(inner, target, kinds, limit)?)?
        }
    };
    for ev in matched.into_iter().take(limit) {
        if let ControlFlow::Break(()) = visitor(&ev) {
            break;
        }
    }
    Ok(())
}

fn collect<'a>(iter: Box<dyn EventIter + 'a>) -> Result<Vec<StoredEvent>, StoreError> {
    iter.collect()
}

// ─── Tombstones ──────────────────────────────────────────────────────────────

pub(super) fn tombstones_for(
    inner: &Arc<Inner>,
    target: &EventId,
) -> Result<Vec<TombstoneRow>, StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    Ok(tombstones::get(inner.tombstones, &txn, target)?.into_iter().collect())
}

pub(super) fn list_tombstones(
    inner: &Arc<Inner>,
) -> Result<Vec<TombstoneRow>, StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    tombstones::list_all(inner.tombstones, &txn)
}

pub(super) fn provenance_for(
    inner: &Arc<Inner>,
    id: &EventId,
) -> Result<Vec<ProvenanceEntry>, StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    provenance::read(inner.provenance, &txn, id)
}

// ─── Watermarks ──────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistWmKey {
    filter_hash: [u8; 32],
    relay_url: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistWmRow {
    key: PersistWmKey,
    synced_up_to: u64,
    last_sync_method: PersistSyncMethod,
    last_negentropy_state: Option<Vec<u8>>,
    bytes_saved_vs_req: u64,
    updated_at: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum PersistSyncMethod {
    Negentropy,
    ReqScan,
    Manual,
}

fn wm_db_key(key: &WatermarkKey) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + key.relay_url.len());
    out.extend_from_slice(&key.filter_hash);
    out.extend_from_slice(key.relay_url.as_bytes());
    out
}

fn encode_row(row: &WatermarkRow) -> Result<Vec<u8>, StoreError> {
    let m = match row.last_sync_method {
        SyncMethod::Negentropy => PersistSyncMethod::Negentropy,
        SyncMethod::ReqScan => PersistSyncMethod::ReqScan,
        SyncMethod::Manual => PersistSyncMethod::Manual,
    };
    let p = PersistWmRow {
        key: PersistWmKey {
            filter_hash: row.key.filter_hash,
            relay_url: row.key.relay_url.clone(),
        },
        synced_up_to: row.synced_up_to,
        last_sync_method: m,
        last_negentropy_state: row.last_negentropy_state.clone(),
        bytes_saved_vs_req: row.bytes_saved_vs_req,
        updated_at: row.updated_at,
    };
    serde_json::to_vec(&p).map_err(|e| StoreError::Encoding(format!("wm enc: {e}")))
}

fn decode_row(bytes: &[u8]) -> Result<WatermarkRow, StoreError> {
    let p: PersistWmRow = serde_json::from_slice(bytes)
        .map_err(|e| StoreError::Encoding(format!("wm dec: {e}")))?;
    let m = match p.last_sync_method {
        PersistSyncMethod::Negentropy => SyncMethod::Negentropy,
        PersistSyncMethod::ReqScan => SyncMethod::ReqScan,
        PersistSyncMethod::Manual => SyncMethod::Manual,
    };
    Ok(WatermarkRow {
        key: WatermarkKey {
            filter_hash: p.key.filter_hash,
            relay_url: p.key.relay_url,
        },
        synced_up_to: p.synced_up_to,
        last_sync_method: m,
        last_negentropy_state: p.last_negentropy_state,
        bytes_saved_vs_req: p.bytes_saved_vs_req,
        updated_at: p.updated_at,
    })
}

pub(super) fn read_watermark(
    inner: &Arc<Inner>,
    key: &WatermarkKey,
) -> Result<Option<WatermarkRow>, StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let k = wm_db_key(key);
    match inner
        .watermarks
        .get(&txn, &k)
        .map_err(|e| StoreError::Io(format!("wm get: {e}")))?
    {
        Some(b) => Ok(Some(decode_row(b)?)),
        None => Ok(None),
    }
}

pub(super) fn write_watermark(
    inner: &Arc<Inner>,
    row: WatermarkRow,
) -> Result<(), StoreError> {
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    let k = wm_db_key(&row.key);
    let b = encode_row(&row)?;
    inner
        .watermarks
        .put(&mut txn, &k, &b)
        .map_err(|e| StoreError::Io(format!("wm put: {e}")))?;
    txn.commit().map_err(|e| StoreError::Io(format!("commit: {e}")))
}

pub(super) fn coverage(
    inner: &Arc<Inner>,
    key: &WatermarkKey,
) -> Result<Coverage, StoreError> {
    let row = read_watermark(inner, key)?;
    let Some(row) = row else {
        return Ok(Coverage::Unknown);
    };
    // Staleness window is coverage policy; defined once next to the `Coverage`
    // type so the mem + lmdb backends cannot drift. D9 caveat (transitional):
    // the `EventStore` trait does not yet thread the kernel clock into the
    // store, so wall-clock is read via a bare `SystemTime::now()` here.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age = now.saturating_sub(row.updated_at);
    if age <= COVERAGE_STALENESS_WINDOW_SECS {
        Ok(Coverage::CompleteAsOf(row.synced_up_to))
    } else {
        Ok(Coverage::PartialUpTo(row.synced_up_to))
    }
}

pub(super) fn list_watermarks_for_relay(
    inner: &Arc<Inner>,
    relay_url: &str,
) -> Result<Vec<WatermarkRow>, StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let mut out = Vec::new();
    for entry in inner
        .watermarks
        .iter(&txn)
        .map_err(|e| StoreError::Io(format!("wm iter: {e}")))?
    {
        let (_k, v) = entry.map_err(|e| StoreError::Io(format!("wm iter step: {e}")))?;
        let row = decode_row(v)?;
        if row.key.relay_url == relay_url {
            out.push(row);
        }
    }
    Ok(out)
}

// Suppress unused warnings for items not yet consumed (RelayUrl from imports).
#[allow(dead_code)]
fn _used(_: RelayUrl) {}
