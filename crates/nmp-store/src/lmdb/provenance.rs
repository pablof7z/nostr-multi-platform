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

// ─── V-52 relay-origin reverse index ─────────────────────────────────────────

/// Separator byte between the `relay_url` and `event_id` segments of a
/// relay-index key.  Relay URLs are valid UTF-8 (`wss://…`) and never contain a
/// NUL byte, so this separator is unambiguous.
const RELAY_INDEX_SEP: u8 = 0x00;

/// Encode a relay-index key as `relay_url || 0x00 || event_id(32)`.
///
/// The relay URL comes first so all entries for one relay form a contiguous
/// prefix range — `list_events_seen_on` is then an O(events-on-relay) range
/// scan over `[relay||0x00 .. relay||0x01)`.
pub(super) fn relay_index_key(relay_url: &str, id: &EventId) -> Vec<u8> {
    let mut k = Vec::with_capacity(relay_url.len() + 1 + 32);
    k.extend_from_slice(relay_url.as_bytes());
    k.push(RELAY_INDEX_SEP);
    k.extend_from_slice(id);
    k
}

/// Inclusive lower / exclusive upper bound for a prefix scan of one relay's
/// entries: `[relay||0x00, relay||0x01)`.
pub(super) fn relay_index_prefix_bounds(relay_url: &str) -> (Vec<u8>, Vec<u8>) {
    let mut lo = Vec::with_capacity(relay_url.len() + 1);
    lo.extend_from_slice(relay_url.as_bytes());
    lo.push(RELAY_INDEX_SEP);
    let mut hi = Vec::with_capacity(relay_url.len() + 1);
    hi.extend_from_slice(relay_url.as_bytes());
    hi.push(RELAY_INDEX_SEP + 1);
    (lo, hi)
}

/// Decode the `event_id` suffix of a relay-index key for a relay whose URL byte
/// length is `relay_url_len`.  Returns `None` if the key is malformed.
pub(super) fn relay_index_id_from_key(key: &[u8], relay_url_len: usize) -> Option<EventId> {
    // Expected layout: relay_url_len bytes + 1 separator + 32 id bytes.
    if key.len() != relay_url_len + 1 + 32 {
        return None;
    }
    let mut id = [0u8; 32];
    id.copy_from_slice(&key[relay_url_len + 1..]);
    Some(id)
}

/// Write a `(relay_url, event_id)` presence entry into the relay index.
pub(super) fn relay_index_put(
    relay_index: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    relay_url: &str,
    id: &EventId,
) -> Result<(), StoreError> {
    let key = relay_index_key(relay_url, id);
    relay_index
        .put(txn, &key, &[])
        .map_err(|e| StoreError::Io(format!("relay_index put: {e}")))
}

/// Remove a single `(relay_url, event_id)` entry from the relay index.
pub(super) fn relay_index_delete_exact(
    relay_index: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    relay_url: &str,
    id: &EventId,
) -> Result<(), StoreError> {
    let key = relay_index_key(relay_url, id);
    relay_index
        .delete(txn, &key)
        .map_err(|e| StoreError::Io(format!("relay_index delete: {e}")))?;
    Ok(())
}

/// Decode the relay-url list from a serialized provenance value.
///
/// Used by the relay-index backfill (`open.rs`) and by `delete` to find every
/// `(relay, id)` entry that must be removed when an event leaves the store.
pub(super) fn decode_relays(bytes: &[u8]) -> Result<Vec<String>, StoreError> {
    Ok(decode(bytes)?.into_iter().map(|e| e.relay_url).collect())
}

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
///
/// **V-52**: also maintains the relay-origin reverse index so it stays a
/// faithful projection of provenance.  Adds the `(relay_url, id)` entry for the
/// incoming relay; if the LRU eviction overwrites an existing non-primary relay
/// (capacity full), the evicted relay's `(relay, id)` index entry is removed.
pub(super) fn upsert(
    db: Database<Bytes, Bytes>,
    relay_index: Database<Bytes, Bytes>,
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
        // Index entry already present (idempotent), but re-assert for safety.
        relay_index_put(relay_index, txn, &relay_url, id)?;
        return Ok(entries.len() as u32);
    }

    // Capacity full → overwrite oldest non-primary entry.
    if entries.len() >= MAX_PROVENANCE_ENTRIES {
        if let Some(oldest) = entries.iter_mut().skip(1).min_by_key(|e| e.last_seen_ms) {
            // The relay being evicted from provenance must also leave the index.
            let evicted_relay = oldest.relay_url.clone();
            *oldest = ProvenanceEntry {
                relay_url: relay_url.clone(),
                first_seen_ms: received_at_ms,
                last_seen_ms: received_at_ms,
                primary: false,
            };
            sort_and_mark(&mut entries);
            let bytes = encode(&entries)?;
            db.put(txn, id, &bytes)
                .map_err(|e| StoreError::Io(format!("prov put: {e}")))?;
            // Index: drop the evicted relay, add the incoming one.  Only drop the
            // evicted entry if no surviving provenance entry still references it
            // (it cannot, since provenance relays are unique, but stay defensive).
            if !entries.iter().any(|e| e.relay_url == evicted_relay) {
                relay_index_delete_exact(relay_index, txn, &evicted_relay, id)?;
            }
            relay_index_put(relay_index, txn, &relay_url, id)?;
            return Ok(entries.len() as u32);
        }
    }

    // Append.
    entries.push(ProvenanceEntry {
        relay_url: relay_url.clone(),
        first_seen_ms: received_at_ms,
        last_seen_ms: received_at_ms,
        primary: false,
    });
    sort_and_mark(&mut entries);
    let bytes = encode(&entries)?;
    db.put(txn, id, &bytes)
        .map_err(|e| StoreError::Io(format!("prov put: {e}")))?;
    relay_index_put(relay_index, txn, &relay_url, id)?;
    Ok(entries.len() as u32)
}

/// Remove the provenance entry for an event id (used on `Replaced` and every
/// event-removal path).
///
/// **V-52**: before deleting the provenance row, every `(relay, id)` entry it
/// references is removed from the relay-origin reverse index so the index never
/// contains dangling references.
pub(super) fn delete(
    db: Database<Bytes, Bytes>,
    relay_index: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    id: &EventId,
) -> Result<(), StoreError> {
    // Remove the reverse-index entries for every relay in this event's
    // provenance before the provenance row itself is dropped.  Decode into an
    // owned Vec first so the read borrow of `txn` ends before the mutable
    // index deletes below.
    let relays: Vec<String> = match db
        .get(txn, id)
        .map_err(|e| StoreError::Io(format!("prov get: {e}")))?
    {
        Some(bytes) => decode_relays(bytes)?,
        None => Vec::new(),
    };
    for relay_url in relays {
        relay_index_delete_exact(relay_index, txn, &relay_url, id)?;
    }
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
