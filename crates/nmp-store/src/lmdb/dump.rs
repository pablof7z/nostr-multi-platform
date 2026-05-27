//! `EventStore::dump` for the LMDB backend.
//!
//! Mirrors `mem::query::dump` line-format and deterministic ordering, so
//! `nmp dump` output is identical across backends — a useful test invariant.

use std::sync::Arc;

use nostr::prelude::*;

use super::{conv, Inner};
use crate::types::{DumpFormat, DumpStats};
use crate::StoreError;

#[derive(serde::Deserialize)]
struct TombShallow {
    deleted_at: u64,
    origin: String,
}

#[derive(serde::Deserialize)]
struct WmShallowKey {
    filter_hash: [u8; 32],
    relay_url: String,
}

#[derive(serde::Deserialize)]
struct WmShallow {
    key: WmShallowKey,
    synced_up_to: u64,
}

pub(super) fn dump(
    inner: &Arc<Inner>,
    out: &mut dyn std::io::Write,
    _format: DumpFormat,
) -> Result<DumpStats, StoreError> {
    let mut stats = DumpStats::default();
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;

    // Events.
    let mut events: Vec<(String, nostr::Event, u64)> = Vec::new();
    let iter = inner
        .lmdb
        .query(&txn, Filter::new())
        .map_err(|e| StoreError::Io(format!("query: {e}")))?;
    for ev in iter {
        let owned: nostr::Event = ev.into_owned();
        let id_hex = owned.id.to_string();
        events.push((id_hex, owned, 0));
    }
    events.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
    for (_id, ev, received_at_ms) in events {
        let raw = conv::nostr_to_raw(&ev)?;
        let line = serde_json::json!({
            "type": "event",
            "event": raw,
            "received_at_ms": received_at_ms,
        })
        .to_string();
        let bytes = (line + "\n").into_bytes();
        stats.bytes_written += bytes.len() as u64;
        out.write_all(&bytes)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        stats.events += 1;
    }

    // Tombstones.
    let mut tomb_pairs: Vec<(String, TombShallow)> = Vec::new();
    for entry in inner
        .tombstones
        .iter(&txn)
        .map_err(|e| StoreError::Io(format!("tomb iter: {e}")))?
    {
        let (k, v) = entry.map_err(|e| StoreError::Io(format!("tomb step: {e}")))?;
        let id_hex: String = k.iter().map(|b| format!("{b:02x}")).collect();
        let row: TombShallow = serde_json::from_slice(v)
            .map_err(|e| StoreError::Encoding(format!("tomb dec: {e}")))?;
        tomb_pairs.push((id_hex, row));
    }
    tomb_pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (id_hex, row) in tomb_pairs {
        let line = serde_json::json!({
            "type": "tombstone",
            "target_id": id_hex,
            "deleted_at": row.deleted_at,
            "origin": row.origin,
        })
        .to_string();
        let bytes = (line + "\n").into_bytes();
        stats.bytes_written += bytes.len() as u64;
        out.write_all(&bytes)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        stats.tombstones += 1;
    }

    // Watermarks.
    let mut wms: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for entry in inner
        .watermarks
        .iter(&txn)
        .map_err(|e| StoreError::Io(format!("wm iter: {e}")))?
    {
        let (k, v) = entry.map_err(|e| StoreError::Io(format!("wm step: {e}")))?;
        wms.push((k.to_vec(), v.to_vec()));
    }
    wms.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (_k, v) in wms {
        let row: WmShallow =
            serde_json::from_slice(&v).map_err(|e| StoreError::Encoding(format!("wm dec: {e}")))?;
        let line = serde_json::json!({
            "type": "watermark",
            "filter_hash": row.key.filter_hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "relay_url": row.key.relay_url,
            "synced_up_to": row.synced_up_to,
        })
        .to_string();
        let bytes = (line + "\n").into_bytes();
        stats.bytes_written += bytes.len() as u64;
        out.write_all(&bytes)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        stats.watermarks += 1;
    }

    // Domain rows.
    let mut dr: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for entry in inner
        .domain_data
        .iter(&txn)
        .map_err(|e| StoreError::Io(format!("dom iter: {e}")))?
    {
        let (k, v) = entry.map_err(|e| StoreError::Io(format!("dom step: {e}")))?;
        dr.push((k.to_vec(), v.to_vec()));
    }
    dr.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (k, v) in dr {
        let split = k.iter().position(|&b| b == 0u8).unwrap_or(k.len());
        let (ns, rest) = k.split_at(split);
        let user_k = if rest.is_empty() { rest } else { &rest[1..] };
        let line = serde_json::json!({
            "type": "domain",
            "namespace": String::from_utf8_lossy(ns),
            "key": user_k,
            "value": v,
        })
        .to_string();
        let bytes = (line + "\n").into_bytes();
        stats.bytes_written += bytes.len() as u64;
        out.write_all(&bytes)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        stats.domain_rows += 1;
    }

    Ok(stats)
}
