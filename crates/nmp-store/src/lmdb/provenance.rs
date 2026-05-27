//! Per-event provenance LRU for the LMDB backend.
//!
//! Matches `mem/mod.rs:149-187` exactly:
//!   * 32-entry cap (`MAX_PROVENANCE_ENTRIES`).
//!   * Existing relay → update first/last seen.
//!   * Capacity full → overwrite oldest non-primary entry.
//!   * Sort by `(first_seen_ms asc, relay_url asc)`; mark index 0 as primary.
//!
//! Encoding: serde_json (the existing dump path already serializes via JSON;
//! provenance is small, no hot-path concern at this scope).

use heed::types::Bytes;
use heed::{Database, RoTxn, RwTxn};

use crate::types::{EventId, ProvenanceEntry, RelayUrl};
use crate::StoreError;

/// Maximum provenance entries kept per event. Mirrors `mem/mod.rs:42`.
pub(super) const MAX_PROVENANCE_ENTRIES: usize = 32;

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistEntry {
    relay_url: String,
    first_seen_ms: u64,
    last_seen_ms: u64,
    primary: bool,
}

impl From<&ProvenanceEntry> for PersistEntry {
    fn from(e: &ProvenanceEntry) -> Self {
        Self {
            relay_url: e.relay_url.clone(),
            first_seen_ms: e.first_seen_ms,
            last_seen_ms: e.last_seen_ms,
            primary: e.primary,
        }
    }
}

impl From<PersistEntry> for ProvenanceEntry {
    fn from(e: PersistEntry) -> Self {
        Self {
            relay_url: e.relay_url,
            first_seen_ms: e.first_seen_ms,
            last_seen_ms: e.last_seen_ms,
            primary: e.primary,
        }
    }
}

pub(super) fn read(
    db: Database<Bytes, Bytes>,
    txn: &RoTxn,
    id: &EventId,
) -> Result<Vec<ProvenanceEntry>, StoreError> {
    match db
        .get(txn, id)
        .map_err(|e| StoreError::Io(format!("prov get: {e}")))?
    {
        Some(bytes) => decode(bytes),
        None => Ok(Vec::new()),
    }
}

fn decode(bytes: &[u8]) -> Result<Vec<ProvenanceEntry>, StoreError> {
    let persisted: Vec<PersistEntry> = serde_json::from_slice(bytes)
        .map_err(|e| StoreError::Encoding(format!("prov decode: {e}")))?;
    Ok(persisted.into_iter().map(Into::into).collect())
}

fn encode(entries: &[ProvenanceEntry]) -> Result<Vec<u8>, StoreError> {
    let persisted: Vec<PersistEntry> = entries.iter().map(PersistEntry::from).collect();
    serde_json::to_vec(&persisted).map_err(|e| StoreError::Encoding(format!("prov encode: {e}")))
}

/// Upsert a provenance entry. Mirrors `mem::upsert_provenance` semantics.
///
/// Returns the post-upsert entry count (used by `InsertOutcome::*.sources_after`).
pub(super) fn upsert(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    id: &EventId,
    relay_url: RelayUrl,
    received_at_ms: u64,
) -> Result<u32, StoreError> {
    let mut entries = read_rw(db, txn, id)?;

    // Existing relay → bump times.
    if let Some(e) = entries.iter_mut().find(|e| e.relay_url == relay_url) {
        if received_at_ms < e.first_seen_ms {
            e.first_seen_ms = received_at_ms;
        }
        if received_at_ms > e.last_seen_ms {
            e.last_seen_ms = received_at_ms;
        }
        sort_and_mark(&mut entries);
        let bytes = encode(&entries)?;
        db.put(txn, id, &bytes)
            .map_err(|e| StoreError::Io(format!("prov put: {e}")))?;
        return Ok(entries.len() as u32);
    }

    // Capacity full → overwrite oldest non-primary entry.
    if entries.len() >= MAX_PROVENANCE_ENTRIES {
        if let Some(oldest) = entries.iter_mut().skip(1).min_by_key(|e| e.last_seen_ms) {
            *oldest = ProvenanceEntry {
                relay_url,
                first_seen_ms: received_at_ms,
                last_seen_ms: received_at_ms,
                primary: false,
            };
            sort_and_mark(&mut entries);
            let bytes = encode(&entries)?;
            db.put(txn, id, &bytes)
                .map_err(|e| StoreError::Io(format!("prov put: {e}")))?;
            return Ok(entries.len() as u32);
        }
    }

    // Append.
    entries.push(ProvenanceEntry {
        relay_url,
        first_seen_ms: received_at_ms,
        last_seen_ms: received_at_ms,
        primary: false,
    });
    sort_and_mark(&mut entries);
    let bytes = encode(&entries)?;
    db.put(txn, id, &bytes)
        .map_err(|e| StoreError::Io(format!("prov put: {e}")))?;
    Ok(entries.len() as u32)
}

/// Remove the provenance entry for an event id (used on `Replaced`).
pub(super) fn delete(
    db: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    id: &EventId,
) -> Result<(), StoreError> {
    db.delete(txn, id)
        .map_err(|e| StoreError::Io(format!("prov delete: {e}")))?;
    Ok(())
}

fn read_rw(
    db: Database<Bytes, Bytes>,
    txn: &RwTxn,
    id: &EventId,
) -> Result<Vec<ProvenanceEntry>, StoreError> {
    match db
        .get(txn, id)
        .map_err(|e| StoreError::Io(format!("prov get: {e}")))?
    {
        Some(bytes) => decode(bytes),
        None => Ok(Vec::new()),
    }
}

fn sort_and_mark(entries: &mut [ProvenanceEntry]) {
    entries.sort_by(|a, b| {
        a.first_seen_ms
            .cmp(&b.first_seen_ms)
            .then(a.relay_url.cmp(&b.relay_url))
    });
    for (i, e) in entries.iter_mut().enumerate() {
        e.primary = i == 0;
    }
}
