//! NMP-side tombstone sub-dbs for the LMDB backend.
//!
//! Two databases:
//!   * `nmp-tombstones` — per-id; key = target_id (32 bytes).
//!   * `nmp-addr-tombstones` — for kind:5 `a`-tag deletes; key = "kind:pk:dtag" bytes.
//!
//! Each row stores the full `TombstoneRow`: kind5 event id, deleter pubkey,
//! `deleted_at`, sources, origin. The upstream fork's `deleted_ids` carries
//! only a presence bit; we keep it in sync (every NMP tombstone write also
//! marks the corresponding fork entry) so the fork's `is_deleted` check
//! continues to fire as a fast pre-filter inside `save_event_with_txn`.

use heed::types::Bytes;
use heed::{Database, RoTxn, RwTxn};

use crate::types::{EventId, RelayUrl, TombstoneOrigin, TombstoneRow};
use crate::StoreError;

// ─── Encoding ────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistRow {
    target_id: [u8; 32],
    kind5_event_id: Option<[u8; 32]>,
    deleter_pubkey: Option<[u8; 32]>,
    deleted_at: u64,
    sources: Vec<String>,
    origin: TombstoneOrigin,
}

impl From<&TombstoneRow> for PersistRow {
    fn from(r: &TombstoneRow) -> Self {
        Self {
            target_id: r.target_id,
            kind5_event_id: r.kind5_event_id,
            deleter_pubkey: r.deleter_pubkey,
            deleted_at: r.deleted_at,
            sources: r.sources.clone(),
            origin: r.origin,
        }
    }
}

impl From<PersistRow> for TombstoneRow {
    fn from(r: PersistRow) -> Self {
        Self {
            target_id: r.target_id,
            kind5_event_id: r.kind5_event_id,
            deleter_pubkey: r.deleter_pubkey,
            deleted_at: r.deleted_at,
            sources: r.sources,
            origin: r.origin,
        }
    }
}

fn encode(row: &TombstoneRow) -> Result<Vec<u8>, StoreError> {
    let p = PersistRow::from(row);
    serde_json::to_vec(&p).map_err(|e| StoreError::Encoding(format!("tomb encode: {e}")))
}

fn decode(bytes: &[u8]) -> Result<TombstoneRow, StoreError> {
    let p: PersistRow = serde_json::from_slice(bytes)
        .map_err(|e| StoreError::Encoding(format!("tomb decode: {e}")))?;
    Ok(p.into())
}

// ─── Per-id ──────────────────────────────────────────────────────────────────

pub(super) fn get(
    db: Database<Bytes, Bytes>,
    txn: &RoTxn,
    target: &EventId,
) -> Result<Option<TombstoneRow>, StoreError> {
    match db
        .get(txn, target)
        .map_err(|e| StoreError::Io(format!("tomb get: {e}")))?
    {
        Some(b) => Ok(Some(decode(b)?)),
        None => Ok(None),
    }
}

fn get_rw(
    db: Database<Bytes, Bytes>,
    txn: &RwTxn,
    target: &EventId,
) -> Result<Option<TombstoneRow>, StoreError> {
    match db
        .get(txn, target)
        .map_err(|e| StoreError::Io(format!("tomb get: {e}")))?
    {
        Some(b) => Ok(Some(decode(b)?)),
        None => Ok(None),
    }
}

pub(super) fn put(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    target: &EventId,
    row: &TombstoneRow,
) -> Result<(), StoreError> {
    let bytes = encode(row)?;
    db.put(txn, target, &bytes)
        .map_err(|e| StoreError::Io(format!("tomb put: {e}")))
}

pub(super) fn delete(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    target: &EventId,
) -> Result<(), StoreError> {
    db.delete(txn, target)
        .map_err(|e| StoreError::Io(format!("tomb del: {e}")))?;
    Ok(())
}

/// Max-merge semantics matching `mem/insert.rs::merge_tombstone`:
///   * `deleted_at` = max(existing, incoming).
///   * `sources` = union (incoming-first preserved if existing missing).
///   * `kind5_event_id` follows the newer `deleted_at`.
pub(super) fn merge_per_id(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    target: &EventId,
    incoming: TombstoneRow,
) -> Result<(), StoreError> {
    match get_rw(db, txn, target)? {
        Some(mut existing) => {
            if incoming.deleted_at > existing.deleted_at {
                existing.deleted_at = incoming.deleted_at;
                existing.kind5_event_id = incoming.kind5_event_id;
            }
            for src in incoming.sources {
                if !existing.sources.contains(&src) {
                    existing.sources.push(src);
                }
            }
            put(db, txn, target, &existing)
        }
        None => put(db, txn, target, &incoming),
    }
}

// ─── Address-keyed (param-replaceable a-tag deletes) ─────────────────────────

pub(super) fn get_addr(
    db: Database<Bytes, Bytes>,
    txn: &RoTxn,
    addr_key: &[u8],
) -> Result<Option<TombstoneRow>, StoreError> {
    match db
        .get(txn, addr_key)
        .map_err(|e| StoreError::Io(format!("addr-tomb get: {e}")))?
    {
        Some(b) => Ok(Some(decode(b)?)),
        None => Ok(None),
    }
}

fn get_addr_rw(
    db: Database<Bytes, Bytes>,
    txn: &RwTxn,
    addr_key: &[u8],
) -> Result<Option<TombstoneRow>, StoreError> {
    match db
        .get(txn, addr_key)
        .map_err(|e| StoreError::Io(format!("addr-tomb get: {e}")))?
    {
        Some(b) => Ok(Some(decode(b)?)),
        None => Ok(None),
    }
}

pub(super) fn merge_addr(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    addr_key: &[u8],
    incoming: TombstoneRow,
) -> Result<(), StoreError> {
    match get_addr_rw(db, txn, addr_key)? {
        Some(mut existing) => {
            if incoming.deleted_at > existing.deleted_at {
                existing.deleted_at = incoming.deleted_at;
                existing.kind5_event_id = incoming.kind5_event_id;
            }
            for src in incoming.sources {
                if !existing.sources.contains(&src) {
                    existing.sources.push(src);
                }
            }
            let bytes = encode(&existing)?;
            db.put(txn, addr_key, &bytes)
                .map_err(|e| StoreError::Io(format!("addr-tomb put: {e}")))
        }
        None => {
            let bytes = encode(&incoming)?;
            db.put(txn, addr_key, &bytes)
                .map_err(|e| StoreError::Io(format!("addr-tomb put: {e}")))
        }
    }
}

pub(super) fn list_all(
    db: Database<Bytes, Bytes>,
    txn: &RoTxn,
) -> Result<Vec<TombstoneRow>, StoreError> {
    let mut out = Vec::new();
    for entry in db
        .iter(txn)
        .map_err(|e| StoreError::Io(format!("tomb iter: {e}")))?
    {
        let (_k, v) = entry.map_err(|e| StoreError::Io(format!("tomb iter step: {e}")))?;
        out.push(decode(v)?);
    }
    Ok(out)
}

// ─── Constructors ────────────────────────────────────────────────────────────

pub(super) fn kind5_row(
    target_id: EventId,
    kind5_id: EventId,
    kind5_pubkey: EventId,
    deleted_at: u64,
    source: &RelayUrl,
) -> TombstoneRow {
    TombstoneRow {
        target_id,
        kind5_event_id: Some(kind5_id),
        deleter_pubkey: Some(kind5_pubkey),
        deleted_at,
        sources: vec![source.clone()],
        origin: TombstoneOrigin::Kind5,
    }
}

/// Compose the address-tombstone key. Matches mem-side format
/// (`mem/insert.rs:81-86`): `"{kind}:{pubkey_hex}:{dtag}"`.
pub(super) fn addr_key(kind: u32, pubkey_hex: &str, dtag_bytes: &[u8]) -> Vec<u8> {
    let mut s = format!("{kind}:{pubkey_hex}:").into_bytes();
    s.extend_from_slice(dtag_bytes);
    s
}
