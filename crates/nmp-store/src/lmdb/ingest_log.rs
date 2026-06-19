//! LMDB ingest-log helpers (ADR-0058 §4).
//!
//! Two sub-dbs:
//!   `nmp-ingest-log`:  seq(8 BE) → JSON(LogEntryPersist)
//!   `nmp-ingest-meta`: ASCII key → 8 BE u64 (keys: "last_seq", "gc_floor")
//!
//! D4: `next_seq` reads+increments `last_seq` INSIDE the caller's `RwTxn`.
//!
//! BLOCKING 4 (append-time trim): every `append_*` calls `trim_in_txn` inside
//! the SAME write txn as the append, advancing `gc_floor` atomically so the log
//! is NEVER unbounded between GC passes. Mirrors Mem's `log_gc_trim` which trims
//! on every `log_append`. The standalone `gc_trim` function is removed — no
//! legacy/parallel path (feedback_no_legacy_parallel_paths.md).
//!
//! SHOULD-FIX 6 (stable persisted schema): `LogEntryPersist` carries a `version`
//! field (current: 1) and all fields/variants are pinned to explicit serde names
//! so a future Rust identifier rename cannot silently break the on-disk format.
//! Reads reject `version > 1`. `#[serde(default)]` on `version` lets old entries
//! (written without the field) be read as version 0, which is accepted.

#![cfg(feature = "lmdb-backend")]

use heed::types::Bytes;
use heed::{Database, RoTxn, RwTxn};

use crate::ingest_log::{
    DeleteReason, LogOp, PullGap, PullPage, ScanLogResult, StoreLogEntry, DEFAULT_LOG_MAX_ENTRIES,
};
use crate::types::{EventId, RawEvent, RelayUrl};
use crate::StoreError;

const KEY_LAST_SEQ: &[u8] = b"last_seq";
const KEY_GC_FLOOR: &[u8] = b"gc_floor";

/// DURABLE on-disk record — serialized as JSON in `nmp-ingest-log`.
///
/// SHOULD-FIX 6: every field is pinned with `#[serde(rename)]` so a Rust
/// identifier rename never silently breaks the on-disk format. The `version`
/// field starts at 1; reads reject `version > 1` (unknown future schema).
/// `#[serde(default)]` on `version` accepts old entries (written before this
/// field existed) as version 0, which is still ≤ 1 and accepted.
#[derive(serde::Serialize, serde::Deserialize)]
struct LogEntryPersist {
    /// On-disk format version. Current: 1. Reject on read if > 1.
    #[serde(default, rename = "version")]
    version: u8,
    #[serde(rename = "op")]
    op: LogOp,
    #[serde(rename = "event_id")]
    event_id: EventId,
    #[serde(rename = "raw_event")]
    raw_event: Option<RawEvent>,
    #[serde(rename = "source_relay")]
    source_relay: Option<String>,
    #[serde(rename = "received_at_ms")]
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
        version: 1,
        op: LogOp::Inserted,
        event_id: *event_id,
        raw_event: Some(raw_event),
        source_relay: Some(source_relay.clone()),
        received_at_ms,
    };
    write_entry(ingest_log, txn, seq, &entry)?;
    // BLOCKING 4: trim inside the same txn so the log is never unbounded.
    trim_in_txn(ingest_log, ingest_meta, txn, seq)?;
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
        version: 1,
        op: LogOp::Replaced { replaced_id },
        event_id: *new_event_id,
        raw_event: Some(raw_event),
        source_relay: Some(source_relay.clone()),
        received_at_ms,
    };
    write_entry(ingest_log, txn, seq, &entry)?;
    // BLOCKING 4: trim inside the same txn so the log is never unbounded.
    trim_in_txn(ingest_log, ingest_meta, txn, seq)?;
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
        version: 1,
        op: LogOp::Deleted { target_id, reason },
        event_id: *carrier_event_id,
        raw_event: None,
        source_relay: None,
        received_at_ms,
    };
    write_entry(ingest_log, txn, seq, &entry)?;
    // BLOCKING 4: trim inside the same txn so the log is never unbounded.
    trim_in_txn(ingest_log, ingest_meta, txn, seq)?;
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
        Some(Ok((k, _))) if k.len() >= 8 => Some(u64::from_be_bytes(k[..8].try_into().unwrap())),
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

    // SHOULD-FIX 5: guard against u64::MAX overflow (never panic before FFI).
    let start_seq = match after_seq.checked_add(1) {
        Some(s) => s,
        None => {
            // after_seq == u64::MAX: no possible entries above it.
            return Ok(ScanLogResult::Page(PullPage {
                entries: vec![],
                next_after_seq: after_seq,
                latest_seq: latest,
                has_more: false,
            }));
        }
    };

    let lower = start_seq.to_be_bytes();
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
        let persist = decode_entry(v)?;
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

// ── Private helpers ──────────────────────────────────────────────────────────

/// Deserialize a log entry and validate its version.
///
/// SHOULD-FIX 6: reject entries with `version > 1` so an upgraded node that
/// wrote a newer schema is detected immediately rather than silently
/// misinterpreted. `version == 0` (entries written before the field was added)
/// is accepted via `#[serde(default)]`.
fn decode_entry(v: &[u8]) -> Result<LogEntryPersist, StoreError> {
    let persist: LogEntryPersist = serde_json::from_slice(v)
        .map_err(|e| StoreError::Encoding(format!("ingest_log decode: {e}")))?;
    if persist.version > 1 {
        return Err(StoreError::Encoding(format!(
            "ingest_log: unknown format version {} (max supported: 1)",
            persist.version
        )));
    }
    Ok(persist)
}

/// Trim the ingest log to at most `DEFAULT_LOG_MAX_ENTRIES` inside the
/// caller's write txn.
///
/// BLOCKING 4: append-time trim means the log is ALWAYS bounded immediately
/// after each append — no separate GC pass required. The `gc_floor` advance is
/// committed atomically with the append so gap-contract invariants hold:
/// `scan_since(after_seq < gc_floor)` returns `Gap { first_available_seq =
/// gc_floor + 1 }`.
fn trim_in_txn(
    ingest_log: Database<Bytes, Bytes>,
    ingest_meta: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    latest_seq: u64,
) -> Result<(), StoreError> {
    let floor = read_u64(ingest_meta, txn, KEY_GC_FLOOR)?;
    let retained = latest_seq.saturating_sub(floor);
    if retained <= DEFAULT_LOG_MAX_ENTRIES {
        return Ok(());
    }
    let to_prune = retained - DEFAULT_LOG_MAX_ENTRIES;
    let new_floor = floor + to_prune;

    let lo = (floor + 1).to_be_bytes();
    let hi_ex = (new_floor + 1).to_be_bytes();
    let range_bounds = (
        std::ops::Bound::Included(lo.as_slice()),
        std::ops::Bound::Excluded(hi_ex.as_slice()),
    );
    // Collect all keys to delete first (shared borrow of txn via range iterator),
    // then delete them (mutable borrow of txn). The block ensures the iterator
    // — and its borrow — is dropped before the mutable borrows below.
    // D6 (fail-loud): propagate every cursor-step error — no .ok() swallow.
    let keys_to_delete: Vec<Vec<u8>> = ingest_log
        .range(&*txn, &range_bounds)
        .map_err(|e| StoreError::Io(format!("ingest_log trim range: {e}")))?
        .map(|r| {
            r.map_err(|e| StoreError::Io(format!("ingest_log trim step: {e}")))
                .map(|(k, _)| k.to_vec())
        })
        .collect::<Result<Vec<_>, _>>()?;
    for k in &keys_to_delete {
        ingest_log
            .delete(txn, k.as_slice())
            .map_err(|e| StoreError::Io(format!("ingest_log trim delete: {e}")))?;
    }
    // Always advance gc_floor to new_floor — even if no physical keys were in
    // the trimmed range (entries may have been absent). The gap contract requires
    // gc_floor to equal new_floor so scan_since returns the correct
    // first_available_seq = gc_floor + 1 regardless of physical key presence.
    write_u64(ingest_meta, txn, KEY_GC_FLOOR, new_floor)?;
    Ok(())
}

fn read_u64_ro(db: Database<Bytes, Bytes>, txn: &RoTxn, key: &[u8]) -> Result<u64, StoreError> {
    match db
        .get(txn, key)
        .map_err(|e| StoreError::Io(format!("ingest_meta get: {e}")))?
    {
        Some(v) if v.len() >= 8 => Ok(u64::from_be_bytes(v[..8].try_into().unwrap())),
        _ => Ok(0),
    }
}

fn read_u64(db: Database<Bytes, Bytes>, txn: &RwTxn, key: &[u8]) -> Result<u64, StoreError> {
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
