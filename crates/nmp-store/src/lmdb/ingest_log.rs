//! LMDB ingest-log helpers (ADR-0058 §4).
//!
//! Two sub-dbs:
//!   `nmp-ingest-log`:  seq(8 BE) → JSON(LogEntryPersist)
//!   `nmp-ingest-meta`: ASCII key → 8 BE u64 (keys: "last_seq", "gc_floor")
//!
//! D4: `next_seq` reads+increments `last_seq` INSIDE the caller's `RwTxn`.

#![cfg(feature = "lmdb-backend")]

use heed::types::Bytes;
use heed::{Database, RoTxn, RwTxn};

use crate::ingest_log::{
    DeleteReason, LogOp, PullGap, PullPage, ScanLogResult, StoreLogEntry,
    DEFAULT_LOG_MAX_ENTRIES,
};
use crate::types::{EventId, RawEvent, RelayUrl};
use crate::StoreError;

const KEY_LAST_SEQ: &[u8] = b"last_seq";
const KEY_GC_FLOOR: &[u8] = b"gc_floor";

#[derive(serde::Serialize, serde::Deserialize)]
struct LogEntryPersist {
    op: LogOp,
    event_id: EventId,
    raw_event: Option<RawEvent>,
    source_relay: Option<String>,
    received_at_ms: u64,
}

/// Allocate next seq inside the caller's RwTxn (D4).
pub(super) fn next_seq(
    ingest_meta: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
) -> Result<u64, StoreError> {
    let current = read_u64(ingest_meta, txn, KEY_LAST_SEQ)?;
    let seq = current + 1;
    write_u64(ingest_meta, txn, KEY_LAST_SEQ, seq)?;
    Ok(seq)
}

pub(super) fn append_inserted(
    ingest_log: Database<Bytes, Bytes>,
    ingest_meta: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    event_id: &EventId,
    raw_event: RawEvent,
    source_relay: &RelayUrl,
    received_at_ms: u64,
) -> Result<u64, StoreError> {
    let seq = next_seq(ingest_meta, txn)?;
    let entry = LogEntryPersist {
        op: LogOp::Inserted,
        event_id: *event_id,
        raw_event: Some(raw_event),
        source_relay: Some(source_relay.clone()),
        received_at_ms,
    };
    write_entry(ingest_log, txn, seq, &entry)?;
    Ok(seq)
}

pub(super) fn append_replaced(
    ingest_log: Database<Bytes, Bytes>,
    ingest_meta: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    new_event_id: &EventId,
    replaced_id: EventId,
    raw_event: RawEvent,
    source_relay: &RelayUrl,
    received_at_ms: u64,
) -> Result<u64, StoreError> {
    let seq = next_seq(ingest_meta, txn)?;
    let entry = LogEntryPersist {
        op: LogOp::Replaced { replaced_id },
        event_id: *new_event_id,
        raw_event: Some(raw_event),
        source_relay: Some(source_relay.clone()),
        received_at_ms,
    };
    write_entry(ingest_log, txn, seq, &entry)?;
    Ok(seq)
}

pub(super) fn append_deleted(
    ingest_log: Database<Bytes, Bytes>,
    ingest_meta: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    carrier_event_id: &EventId,
    target_id: EventId,
    reason: DeleteReason,
    received_at_ms: u64,
) -> Result<u64, StoreError> {
    let seq = next_seq(ingest_meta, txn)?;
    let entry = LogEntryPersist {
        op: LogOp::Deleted { target_id, reason },
        event_id: *carrier_event_id,
        raw_event: None,
        source_relay: None,
        received_at_ms,
    };
    write_entry(ingest_log, txn, seq, &entry)?;
    Ok(seq)
}

pub(super) fn latest_seq(
    ingest_meta: Database<Bytes, Bytes>,
    env: &heed::Env,
) -> Result<u64, StoreError> {
    let txn = env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("ingest_log latest read_txn: {e}")))?;
    read_u64_ro(ingest_meta, &txn, KEY_LAST_SEQ)
}

pub(super) fn oldest_seq(
    ingest_log: Database<Bytes, Bytes>,
    ingest_meta: Database<Bytes, Bytes>,
    env: &heed::Env,
) -> Result<Option<u64>, StoreError> {
    let txn = env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("ingest_log oldest read_txn: {e}")))?;
    let last = read_u64_ro(ingest_meta, &txn, KEY_LAST_SEQ)?;
    if last == 0 {
        return Ok(None);
    }
    // Scan for the actual first key present.
    // NOTE: split into named `iter` to avoid a temporary `ControlFlow<_, RoRange<_>>`
    // that would hold a borrow of `txn` past its drop point (E0597).
    let lo = 1u64.to_be_bytes();
    let range = (
        std::ops::Bound::Included(lo.as_slice()),
        std::ops::Bound::Unbounded,
    );
    let mut iter = ingest_log
        .range(&txn, &range)
        .map_err(|e| StoreError::Io(format!("ingest_log oldest range: {e}")))?;
    let first: Option<u64> = match iter.next() {
        Some(Ok((k, _))) if k.len() >= 8 => {
            Some(u64::from_be_bytes(k[..8].try_into().unwrap()))
        }
        _ => None,
    };
    drop(iter);
    Ok(first)
}

pub(super) fn scan_since(
    ingest_log: Database<Bytes, Bytes>,
    ingest_meta: Database<Bytes, Bytes>,
    env: &heed::Env,
    after_seq: u64,
    limit: usize,
) -> Result<ScanLogResult, StoreError> {
    let txn = env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("ingest_log scan read_txn: {e}")))?;

    let latest = read_u64_ro(ingest_meta, &txn, KEY_LAST_SEQ)?;
    let gc_floor = read_u64_ro(ingest_meta, &txn, KEY_GC_FLOOR)?;

    if gc_floor > 0 && after_seq < gc_floor {
        return Ok(ScanLogResult::Gap(PullGap {
            requested_after_seq: after_seq,
            first_available_seq: gc_floor + 1,
        }));
    }

    let lower = (after_seq + 1).to_be_bytes();
    let range = (
        std::ops::Bound::Included(lower.as_slice()),
        std::ops::Bound::Unbounded,
    );
    let mut entries: Vec<StoreLogEntry> = Vec::new();
    for item in ingest_log
        .range(&txn, &range)
        .map_err(|e| StoreError::Io(format!("ingest_log range: {e}")))?
    {
        let (k, v) = item.map_err(|e| StoreError::Io(format!("ingest_log step: {e}")))?;
        if k.len() < 8 {
            continue;
        }
        let seq = u64::from_be_bytes(k[..8].try_into().unwrap());
        let persist: LogEntryPersist = serde_json::from_slice(v)
            .map_err(|e| StoreError::Encoding(format!("ingest_log decode: {e}")))?;
        entries.push(StoreLogEntry {
            seq,
            op: persist.op,
            event_id: persist.event_id,
            raw_event: persist.raw_event,
            source_relay: persist.source_relay,
            received_at_ms: persist.received_at_ms,
        });
        if entries.len() >= limit {
            break;
        }
    }

    let next_after_seq = entries.last().map(|e| e.seq).unwrap_or(after_seq);
    let has_more = next_after_seq < latest;
    Ok(ScanLogResult::Page(PullPage {
        entries,
        next_after_seq,
        latest_seq: latest,
        has_more,
    }))
}

pub(super) fn gc_trim(
    ingest_log: Database<Bytes, Bytes>,
    ingest_meta: Database<Bytes, Bytes>,
    env: &heed::Env,
) -> Result<usize, StoreError> {
    let (last, floor) = {
        let txn = env
            .read_txn()
            .map_err(|e| StoreError::Io(format!("ingest_log gc read_txn: {e}")))?;
        let l = read_u64_ro(ingest_meta, &txn, KEY_LAST_SEQ)?;
        let f = read_u64_ro(ingest_meta, &txn, KEY_GC_FLOOR)?;
        (l, f)
    };

    if last == 0 {
        return Ok(0);
    }

    let retained = last.saturating_sub(floor);
    if retained <= DEFAULT_LOG_MAX_ENTRIES {
        return Ok(0);
    }

    let to_prune = (retained - DEFAULT_LOG_MAX_ENTRIES) as usize;
    let new_floor = floor + to_prune as u64;

    let lo = (floor + 1).to_be_bytes();
    let hi_exclusive = (new_floor + 1).to_be_bytes();
    let range = (
        std::ops::Bound::Included(lo.as_slice()),
        std::ops::Bound::Excluded(hi_exclusive.as_slice()),
    );

    let mut txn = env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("ingest_log gc write_txn: {e}")))?;

    let keys_to_delete: Vec<Vec<u8>> = ingest_log
        .range(&txn, &range)
        .map_err(|e| StoreError::Io(format!("ingest_log gc range: {e}")))?
        .filter_map(|r| r.ok().map(|(k, _)| k.to_vec()))
        .collect();
    let trimmed = keys_to_delete.len();
    for k in &keys_to_delete {
        ingest_log
            .delete(&mut txn, k.as_slice())
            .map_err(|e| StoreError::Io(format!("ingest_log gc delete: {e}")))?;
    }
    if trimmed > 0 {
        write_u64(ingest_meta, &mut txn, KEY_GC_FLOOR, new_floor)?;
    }
    txn.commit()
        .map_err(|e| StoreError::Io(format!("ingest_log gc commit: {e}")))?;
    Ok(trimmed)
}

// ── Private helpers ──────────────────────────────────────────────────────────

fn read_u64_ro(
    db: Database<Bytes, Bytes>,
    txn: &RoTxn,
    key: &[u8],
) -> Result<u64, StoreError> {
    match db
        .get(txn, key)
        .map_err(|e| StoreError::Io(format!("ingest_meta get: {e}")))?
    {
        Some(v) if v.len() >= 8 => Ok(u64::from_be_bytes(v[..8].try_into().unwrap())),
        _ => Ok(0),
    }
}

fn read_u64(
    db: Database<Bytes, Bytes>,
    txn: &RwTxn,
    key: &[u8],
) -> Result<u64, StoreError> {
    match db
        .get(txn, key)
        .map_err(|e| StoreError::Io(format!("ingest_meta get: {e}")))?
    {
        Some(v) if v.len() >= 8 => Ok(u64::from_be_bytes(v[..8].try_into().unwrap())),
        _ => Ok(0),
    }
}

fn write_u64(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    key: &[u8],
    value: u64,
) -> Result<(), StoreError> {
    db.put(txn, key, &value.to_be_bytes())
        .map_err(|e| StoreError::Io(format!("ingest_meta put: {e}")))
}

fn write_entry(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    seq: u64,
    entry: &LogEntryPersist,
) -> Result<(), StoreError> {
    let key = seq.to_be_bytes();
    let value = serde_json::to_vec(entry)
        .map_err(|e| StoreError::Encoding(format!("ingest_log encode: {e}")))?;
    db.put(txn, &key, &value)
        .map_err(|e| StoreError::Io(format!("ingest_log put: {e}")))
}
