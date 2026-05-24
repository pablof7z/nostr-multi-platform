//! Read / scan / watermark / dump methods for `MemEventStore`.
//!
//! These are pure reads; all state mutation lives in `insert.rs` and `gc.rs`.
//!
//! ⚠️ See the module-level performance warning in `mem/mod.rs`: every scan
//! below is an O(N) full-table scan — this backend is for tests only.
//!
//! # Performance characteristics
//!
//! **All scans are O(N) full table scans followed by O(N log N) sort.** The
//! `EventStore` trait advertises named indexes, but this backend has none —
//! every query iterates the entire event map, sorts, and truncates.
//!
//! This is acceptable for tests and small WASM builds (the intended use cases).
//! For production workloads, use the LMDB backend (`--features lmdb-backend`),
//! which has real B-tree indexes.
//!
//! Replaceable-event supersession (`handle_supersession`) is likewise O(N) per
//! insert. Do not use this backend for high-throughput relay connections.

use std::ops::ControlFlow;

use super::{bytes_to_hex, MemEventStore};
use crate::events::EventIter;
use crate::types::{
    Coverage, DumpFormat, DumpStats, EventId, ProvenanceEntry, PubKey, StoreQuery,
    StoredEvent, TombstoneRow, WatermarkKey, WatermarkRow, COVERAGE_STALENESS_WINDOW_SECS,
};
use crate::StoreError;

// ─── Primary lookups ─────────────────────────────────────────────────────────

pub(super) fn get_by_id(
    store: &MemEventStore,
    id: &EventId,
) -> Result<Option<StoredEvent>, StoreError> {
    let hex = bytes_to_hex(id);
    let st = store.lock()?;
    Ok(st.events.get(&hex).cloned())
}

pub(super) fn scan_by_author_kind<'a>(
    store: &'a MemEventStore,
    author: &PubKey,
    kinds: &[u32],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let author_hex = bytes_to_hex(author);
    let st = store.lock()?;
    let mut results: Vec<StoredEvent> = st
        .events
        .values()
        .filter(|ev| {
            ev.raw.pubkey == author_hex
                && kinds.contains(&ev.raw.kind)
                && since.is_none_or(|s| ev.raw.created_at >= s)
                && until.is_none_or(|u| ev.raw.created_at <= u)
        })
        .cloned()
        .collect();
    results.sort_by(|a, b| {
        b.raw.created_at
            .cmp(&a.raw.created_at)
            .then(a.raw.id.cmp(&b.raw.id))
    });
    results.truncate(limit);
    Ok(Box::new(results.into_iter().map(Ok)))
}

pub(super) fn get_param_replaceable(
    store: &MemEventStore,
    pubkey: &PubKey,
    kind: u32,
    d_tag: &[u8],
) -> Result<Option<StoredEvent>, StoreError> {
    let pubkey_hex = bytes_to_hex(pubkey);
    let d_str = String::from_utf8_lossy(d_tag).into_owned();
    let st = store.lock()?;
    let winner = st
        .events
        .values()
        .filter(|ev| {
            ev.raw.pubkey == pubkey_hex
                && ev.raw.kind == kind
                && ev.raw
                    .d_tag()
                    .is_some_and(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
        })
        .max_by(|a, b| {
            a.raw.created_at
                .cmp(&b.raw.created_at)
                .then(b.raw.id.cmp(&a.raw.id))
        })
        .cloned();
    Ok(winner)
}

pub(super) fn scan_by_kind_dtag<'a>(
    store: &'a MemEventStore,
    kind: u32,
    d_tag: &[u8],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let d_str = String::from_utf8_lossy(d_tag).into_owned();
    let st = store.lock()?;
    let mut results: Vec<StoredEvent> = st
        .events
        .values()
        .filter(|ev| {
            ev.raw.kind == kind
                && ev.raw
                    .d_tag()
                    .is_some_and(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
                && since.is_none_or(|s| ev.raw.created_at >= s)
                && until.is_none_or(|u| ev.raw.created_at <= u)
        })
        .cloned()
        .collect();
    results.sort_by(|a, b| {
        b.raw.created_at
            .cmp(&a.raw.created_at)
            .then(a.raw.id.cmp(&b.raw.id))
    });
    results.truncate(limit);
    Ok(Box::new(results.into_iter().map(Ok)))
}

pub(super) fn scan_by_etag<'a>(
    store: &'a MemEventStore,
    target: &EventId,
    kinds: &[u32],
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let target_hex = bytes_to_hex(target);
    let st = store.lock()?;
    let mut results: Vec<StoredEvent> = st
        .events
        .values()
        .filter(|ev| {
            kinds.contains(&ev.raw.kind) && ev.raw.e_tags().contains(&target_hex)
        })
        .cloned()
        .collect();
    results.sort_by(|a, b| {
        b.raw.created_at
            .cmp(&a.raw.created_at)
            .then(a.raw.id.cmp(&b.raw.id))
    });
    results.truncate(limit);
    Ok(Box::new(results.into_iter().map(Ok)))
}

pub(super) fn scan_by_ptag<'a>(
    store: &'a MemEventStore,
    target: &PubKey,
    kinds: &[u32],
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let target_hex = bytes_to_hex(target);
    let st = store.lock()?;
    let mut results: Vec<StoredEvent> = st
        .events
        .values()
        .filter(|ev| {
            kinds.contains(&ev.raw.kind) && ev.raw.p_tags().contains(&target_hex)
        })
        .cloned()
        .collect();
    results.sort_by(|a, b| {
        b.raw.created_at
            .cmp(&a.raw.created_at)
            .then(a.raw.id.cmp(&b.raw.id))
    });
    results.truncate(limit);
    Ok(Box::new(results.into_iter().map(Ok)))
}

pub(super) fn scan_by_kind_time<'a>(
    store: &'a MemEventStore,
    kinds: &[u32],
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let st = store.lock()?;
    let mut results: Vec<StoredEvent> = st
        .events
        .values()
        .filter(|ev| {
            (kinds.is_empty() || kinds.contains(&ev.raw.kind))
                && since.is_none_or(|s| ev.raw.created_at >= s)
                && until.is_none_or(|u| ev.raw.created_at <= u)
        })
        .cloned()
        .collect();
    results.sort_by(|a, b| {
        b.raw.created_at
            .cmp(&a.raw.created_at)
            .then(a.raw.id.cmp(&b.raw.id))
    });
    results.truncate(limit);
    Ok(Box::new(results.into_iter().map(Ok)))
}

/// Predicate: does `ev` match `query`? Mirrors the per-index `filter`
/// closures in the `scan_by_*` functions so `query_visit` exercises exactly
/// the same matching logic (no duplicated index semantics).
fn matches(ev: &StoredEvent, query: &StoreQuery) -> bool {
    let in_range = |since: Option<u64>, until: Option<u64>| {
        since.is_none_or(|s| ev.raw.created_at >= s)
            && until.is_none_or(|u| ev.raw.created_at <= u)
    };
    match query {
        StoreQuery::AuthorKind { author, kinds, since, until } => {
            ev.raw.pubkey == bytes_to_hex(author)
                && kinds.contains(&ev.raw.kind)
                && in_range(*since, *until)
        }
        StoreQuery::KindTime { kinds, since, until } => {
            (kinds.is_empty() || kinds.contains(&ev.raw.kind))
                && in_range(*since, *until)
        }
        StoreQuery::KindDtag { kind, d_tag, since, until } => {
            let want = String::from_utf8_lossy(d_tag).into_owned();
            ev.raw.kind == *kind
                && ev.raw
                    .d_tag()
                    .is_some_and(|d| String::from_utf8_lossy(&d).into_owned() == want)
                && in_range(*since, *until)
        }
        StoreQuery::Etag { target, kinds } => {
            kinds.contains(&ev.raw.kind)
                && ev.raw.e_tags().contains(&bytes_to_hex(target))
        }
        StoreQuery::Ptag { target, kinds } => {
            kinds.contains(&ev.raw.kind)
                && ev.raw.p_tags().contains(&bytes_to_hex(target))
        }
    }
}

/// Optimized visitor scan for the memory backend.
///
/// The only allocation is a one-time `Vec<&StoredEvent>` of *references* used
/// to apply the newest-first ordering the index would provide on disk. The
/// visit path itself performs **zero per-event allocation** — the visitor
/// receives a borrow of the stored event and the scan stops the instant the
/// visitor returns [`ControlFlow::Break`] (D8: bounded working set, no
/// per-event alloc after warmup).
pub(super) fn query_visit(
    store: &MemEventStore,
    query: &StoreQuery,
    limit: usize,
    visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
) -> Result<(), StoreError> {
    if limit == 0 {
        return Ok(());
    }
    let st = store.lock()?;
    if limit == 1 {
        let newest = st.events.values().filter(|ev| matches(ev, query)).max_by(|a, b| {
            a.raw.created_at
                .cmp(&b.raw.created_at)
                .then(b.raw.id.cmp(&a.raw.id))
        });
        if let Some(ev) = newest {
            let _ = visitor(ev);
        }
        return Ok(());
    }
    // Prep alloc (one Vec of borrows), not a per-event clone.
    let mut matched: Vec<&StoredEvent> =
        st.events.values().filter(|ev| matches(ev, query)).collect();
    matched.sort_by(|a, b| {
        b.raw.created_at
            .cmp(&a.raw.created_at)
            .then(a.raw.id.cmp(&b.raw.id))
    });
    for ev in matched.into_iter().take(limit) {
        if let ControlFlow::Break(()) = visitor(ev) {
            break;
        }
    }
    Ok(())
}

pub(super) fn scan_expiring_before<'a>(
    store: &'a MemEventStore,
    unix_seconds: u64,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let st = store.lock()?;
    // Ascending by expiration.
    let mut pairs: Vec<(u64, StoredEvent)> = st
        .events
        .values()
        .filter_map(|ev| {
            ev.raw
                .expiration()
                .filter(|&exp| exp < unix_seconds)
                .map(|exp| (exp, ev.clone()))
        })
        .collect();
    pairs.sort_by_key(|(exp, _)| *exp);
    pairs.truncate(limit);
    Ok(Box::new(pairs.into_iter().map(|(_, ev)| Ok(ev))))
}

pub(super) fn tombstones_for(
    store: &MemEventStore,
    target: &EventId,
) -> Result<Vec<TombstoneRow>, StoreError> {
    let hex = bytes_to_hex(target);
    let st = store.lock()?;
    Ok(st.tombstones.get(&hex).cloned().into_iter().collect())
}

pub(super) fn list_tombstones<'a>(
    store: &'a MemEventStore,
) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError> {
    let st = store.lock()?;
    let rows: Vec<TombstoneRow> = st.tombstones.values().cloned().collect();
    Ok(Box::new(rows.into_iter().map(Ok)))
}

pub(super) fn provenance_for(
    store: &MemEventStore,
    id: &EventId,
) -> Result<Vec<ProvenanceEntry>, StoreError> {
    let hex = bytes_to_hex(id);
    let st = store.lock()?;
    Ok(st.provenance.get(&hex).cloned().unwrap_or_default())
}

// ─── Watermarks ──────────────────────────────────────────────────────────────

pub(super) fn read_watermark(
    store: &MemEventStore,
    key: &WatermarkKey,
) -> Result<Option<WatermarkRow>, StoreError> {
    let st = store.lock()?;
    let wm_key = (
        bytes_to_hex(&key.filter_hash),
        key.relay_url.clone(),
    );
    Ok(st.watermarks.get(&wm_key).cloned())
}

pub(super) fn write_watermark(
    store: &MemEventStore,
    row: WatermarkRow,
) -> Result<(), StoreError> {
    let mut st = store.lock()?;
    let wm_key = (bytes_to_hex(&row.key.filter_hash), row.key.relay_url.clone());
    st.watermarks.insert(wm_key, row);
    Ok(())
}

pub(super) fn coverage(
    store: &MemEventStore,
    key: &WatermarkKey,
) -> Result<Coverage, StoreError> {
    let row = read_watermark(store, key)?;
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

pub(super) fn list_watermarks_for_relay<'a>(
    store: &'a MemEventStore,
    relay_url: &str,
) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
{
    let st = store.lock()?;
    let rows: Vec<WatermarkRow> = st
        .watermarks
        .values()
        .filter(|r| r.key.relay_url == relay_url)
        .cloned()
        .collect();
    Ok(Box::new(rows.into_iter().map(Ok)))
}

// ─── Dump ────────────────────────────────────────────────────────────────────

pub(super) fn dump(
    store: &MemEventStore,
    out: &mut dyn std::io::Write,
    _format: DumpFormat,
) -> Result<DumpStats, StoreError> {
    let st = store.lock()?;
    let mut stats = DumpStats::default();

    // Dump events in deterministic order (ascending hex id).
    let mut event_ids: Vec<&String> = st.events.keys().collect();
    event_ids.sort();
    for id in event_ids {
        let ev = &st.events[id];
        let line = serde_json::json!({
            "type": "event",
            "event": *ev.raw,
            "received_at_ms": ev.received_at_ms,
        })
        .to_string();
        let bytes = (line + "\n").into_bytes();
        stats.bytes_written += bytes.len() as u64;
        out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
        stats.events += 1;
    }

    // Dump tombstones in deterministic order.
    let mut tomb_ids: Vec<&String> = st.tombstones.keys().collect();
    tomb_ids.sort();
    for id in tomb_ids {
        let t = &st.tombstones[id];
        let line = serde_json::json!({
            "type": "tombstone",
            "target_id": bytes_to_hex(&t.target_id),
            "deleted_at": t.deleted_at,
            "origin": format!("{:?}", t.origin),
        })
        .to_string();
        let bytes = (line + "\n").into_bytes();
        stats.bytes_written += bytes.len() as u64;
        out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
        stats.tombstones += 1;
    }

    // Dump watermarks in deterministic order.
    let mut wm_keys: Vec<&(String, String)> = st.watermarks.keys().collect();
    wm_keys.sort();
    for k in wm_keys {
        let r = &st.watermarks[k];
        let line = serde_json::json!({
            "type": "watermark",
            "filter_hash": &r.key.filter_hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "relay_url": &r.key.relay_url,
            "synced_up_to": r.synced_up_to,
        })
        .to_string();
        let bytes = (line + "\n").into_bytes();
        stats.bytes_written += bytes.len() as u64;
        out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
        stats.watermarks += 1;
    }

    // Dump domain rows in deterministic order (namespace, key).
    let mut ns_list: Vec<&&'static str> = st.domain_data.keys().collect();
    ns_list.sort();
    for ns in ns_list {
        let data = st.domain_data[ns]
            .lock()
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let mut pairs: Vec<(&Vec<u8>, &Vec<u8>)> = data.iter().collect();
        pairs.sort_by_key(|(k, _)| *k);
        for (k, v) in pairs {
            let line = serde_json::json!({
                "type": "domain",
                "namespace": ns,
                "key": k,
                "value": v,
            })
            .to_string();
            let bytes = (line + "\n").into_bytes();
            stats.bytes_written += bytes.len() as u64;
            out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
            stats.domain_rows += 1;
        }
    }

    Ok(stats)
}
