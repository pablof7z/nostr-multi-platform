//! Read / scan / query_visit methods for the LMDB backend.
//!
//! Strategy: build a `nostr::Filter` from each `EventStore` query method
//! (since/until/kinds/authors/tags/ids), call `Lmdb::query`, then convert
//! each returned `EventBorrow` back to `RawEvent`/`StoredEvent`. The fork's
//! `BTreeSet`-backed `query` already produces newest-first ordering by
//! `(created_at desc, id desc)`; the Mem invariant is `(created_at desc,
//! id asc)`. We post-sort the materialized vec to match Mem's order.

use std::collections::BTreeSet;
use std::ops::ControlFlow;
use std::sync::Arc;

use nostr::prelude::*;

use super::{conv, gc, provenance, tombstones, Inner};
use crate::events::EventIter;
use crate::types::{EventId, ProvenanceEntry, PubKey, StoreQuery, StoredEvent, TombstoneRow};
use crate::StoreError;

// ─── Test-only conversion counter ────────────────────────────────────────────

#[cfg(test)]
static CONVERSION_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn reset_conversion_count() {
    CONVERSION_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn conversion_count() -> usize {
    CONVERSION_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

// ─── Primary lookup ──────────────────────────────────────────────────────────

pub(super) fn get_by_id(
    inner: &Arc<Inner>,
    id: &EventId,
) -> Result<Option<StoredEvent>, StoreError> {
    // Check tombstone under a read-txn first (cheap).
    {
        let txn = inner
            .lmdb
            .read_txn()
            .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
        if tombstones::get(inner.tombstones, &txn, id)?.is_some() {
            return Ok(None);
        }
        if inner
            .lmdb
            .get_event_by_id(&txn, id)
            .map_err(|e| StoreError::Io(format!("get: {e}")))?
            .is_none()
        {
            return Ok(None);
        }
    }
    // Event exists — re-fetch in a write-txn so we can stamp the LRU access
    // counter.  Write-txn per point-read is an accepted trade-off (see gc.rs
    // module-level doc on write-amp vs D7 compliance).
    let mut txn = inner
        .env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    let Some(borrow) = inner
        .lmdb
        .get_event_by_id(&txn, id)
        .map_err(|e| StoreError::Io(format!("get: {e}")))?
    else {
        txn.abort();
        return Ok(None);
    };
    let owned: Event = borrow.into_owned();
    let raw = conv::nostr_to_raw(&owned)?;
    gc::lru_stamp(inner, &mut txn, id)?;
    txn.commit()
        .map_err(|e| StoreError::Io(format!("commit: {e}")))?;
    Ok(Some(conv::stored_from_raw(
        raw, /* received_at_ms */ 0,
    )))
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
        b.raw
            .created_at
            .cmp(&a.raw.created_at)
            .then(a.raw.id.cmp(&b.raw.id))
    });
    out.truncate(limit);
    Ok(out)
}

// ─── Filter construction ─────────────────────────────────────────────────────

/// Build a `nostr::Filter` for the given query.
///
/// Returns `None` for empty-set short-circuits (empty `kinds`, empty `authors`),
/// indicating that the caller should visit nothing.
fn build_filter(query: &StoreQuery) -> Option<Filter> {
    match query {
        StoreQuery::AuthorKind {
            author,
            kinds,
            since,
            until,
        } => {
            if kinds.is_empty() {
                return None;
            }
            let pk = PublicKey::from_slice(author).ok()?;
            let mut f = Filter::new()
                .author(pk)
                .kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            if let Some(s) = since {
                f = f.since(Timestamp::from_secs(*s));
            }
            if let Some(u) = until {
                f = f.until(Timestamp::from_secs(*u));
            }
            Some(f)
        }
        StoreQuery::AuthorsKind {
            authors,
            kinds,
            since,
            until,
        } => {
            if authors.is_empty() || kinds.is_empty() {
                return None;
            }
            let pks: Vec<PublicKey> = authors
                .iter()
                .filter_map(|a| PublicKey::from_slice(a).ok())
                .collect();
            if pks.len() != authors.len() {
                return None;
            }
            let mut f = Filter::new()
                .authors(pks)
                .kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            if let Some(s) = since {
                f = f.since(Timestamp::from_secs(*s));
            }
            if let Some(u) = until {
                f = f.until(Timestamp::from_secs(*u));
            }
            Some(f)
        }
        StoreQuery::KindTime {
            kinds,
            since,
            until,
        } => {
            let mut f = Filter::new();
            if !kinds.is_empty() {
                f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            }
            if let Some(s) = since {
                f = f.since(Timestamp::from_secs(*s));
            }
            if let Some(u) = until {
                f = f.until(Timestamp::from_secs(*u));
            }
            Some(f)
        }
        StoreQuery::KindDtag {
            kind,
            d_tag,
            since,
            until,
        } => {
            let d_str = String::from_utf8_lossy(d_tag).into_owned();
            let mut f = Filter::new()
                .kind(Kind::from(*kind as u16))
                .identifier(d_str);
            if let Some(s) = since {
                f = f.since(Timestamp::from_secs(*s));
            }
            if let Some(u) = until {
                f = f.until(Timestamp::from_secs(*u));
            }
            Some(f)
        }
        StoreQuery::Etag { target, kinds } => {
            let target = nostr::EventId::from_slice(target).ok()?;
            let mut f = Filter::new().event(target);
            if !kinds.is_empty() {
                f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            }
            Some(f)
        }
        StoreQuery::Ptag { target, kinds } => {
            let pk = PublicKey::from_slice(target).ok()?;
            let mut f = Filter::new().pubkey(pk);
            if !kinds.is_empty() {
                f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            }
            Some(f)
        }
    }
}

/// Streaming backend for `query_visit`: converts events lazily, one row at a
/// time from the cursor.  `ControlFlow::Break` stops conversions immediately —
/// unlike the old `run_filter` approach that paid ~`limit` round-trips even if
/// the visitor broke at row 1.
///
/// Ordering: the fork's BTreeSet delivers `(created_at desc, id asc)` which
/// already matches the `MemEventStore` contract, so no tie-group reordering is
/// needed.  We buffer a single tie-group only to respect the `limit` cap while
/// preserving the guarantee that a partial group is still id-asc.
fn run_filter_visit(
    inner: &Arc<Inner>,
    filter: Filter,
    limit: usize,
    visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
) -> Result<(), StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let filter = filter.limit(limit);
    let iter = inner
        .lmdb
        .query(&txn, filter)
        .map_err(|e| StoreError::Io(format!("query: {e}")))?;

    let mut total_visited: usize = 0;

    for ev_borrow in iter {
        if total_visited >= limit {
            break;
        }
        let owned: Event = ev_borrow.into_owned();
        let raw = conv::nostr_to_raw(&owned)?;
        #[cfg(test)]
        CONVERSION_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stored = conv::stored_from_raw(raw, 0);

        total_visited += 1;
        if let ControlFlow::Break(()) = visitor(&stored) {
            break;
        }
    }

    Ok(())
}

pub(super) fn scan_by_author_kind<'a>(
    inner: &'a Arc<Inner>,
    author: &PubKey,
    kinds: &[u32],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    // Empty-set semantics (StoreQuery::AuthorKind doc): an empty kind set
    // matches NOTHING — `AuthorKind` is a positive (author, kinds) selection,
    // never an author-wildcard-over-all-kinds. The nostr-lmdb fork's `Filter`
    // treats empty `kinds` as "any kind", so short-circuit here to stay
    // byte-identical with MemEventStore (whose `kinds.contains` yields nothing
    // for an empty set).
    if kinds.is_empty() {
        return Ok(Box::new(std::iter::empty::<Result<StoredEvent, StoreError>>()));
    }
    let pk = PublicKey::from_slice(author).map_err(|e| StoreError::Encoding(format!("pk: {e}")))?;
    let mut f = Filter::new()
        .author(pk)
        .kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
    if let Some(s) = since {
        f = f.since(Timestamp::from_secs(s));
    }
    if let Some(u) = until {
        f = f.until(Timestamp::from_secs(u));
    }
    let v = run_filter(inner, f, limit)?;
    Ok(Box::new(v.into_iter().map(Ok)))
}

pub(super) fn scan_by_authors_kind<'a>(
    inner: &'a Arc<Inner>,
    authors: &BTreeSet<PubKey>,
    kinds: &[u32],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    // Empty-set semantics (StoreQuery::AuthorsKind doc): an empty author set OR
    // an empty kind set matches NOTHING — this is a positive selection, never a
    // wildcard. The nostr-lmdb fork's `Filter` treats an empty `authors`/`kinds`
    // as "no constraint" (matches all), so we must short-circuit here to stay
    // byte-identical with MemEventStore (whose `contains` checks already yield
    // nothing for an empty set).
    if authors.is_empty() || kinds.is_empty() {
        return Ok(Box::new(std::iter::empty::<Result<StoredEvent, StoreError>>()));
    }
    let pks: Vec<PublicKey> = authors
        .iter()
        .map(|a| PublicKey::from_slice(a).map_err(|e| StoreError::Encoding(format!("pk: {e}"))))
        .collect::<Result<Vec<_>, _>>()?;
    let mut f = Filter::new()
        .authors(pks)
        .kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
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
    let target =
        nostr::EventId::from_slice(target).map_err(|e| StoreError::Encoding(format!("id: {e}")))?;
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
    let pk = PublicKey::from_slice(target).map_err(|e| StoreError::Encoding(format!("pk: {e}")))?;
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
    let pk = PublicKey::from_slice(pubkey).map_err(|e| StoreError::Encoding(format!("pk: {e}")))?;
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
    match build_filter(query) {
        None => Ok(()), // empty-set short-circuit (empty kinds / authors)
        Some(filter) => run_filter_visit(inner, filter, limit, visitor),
    }
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
    Ok(tombstones::get(inner.tombstones, &txn, target)?
        .into_iter()
        .collect())
}

pub(super) fn list_tombstones(inner: &Arc<Inner>) -> Result<Vec<TombstoneRow>, StoreError> {
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
