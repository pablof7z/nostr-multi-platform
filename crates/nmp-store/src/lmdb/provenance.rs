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

use super::open_error::classify_heed_err;
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
    map_size: usize,
    max_readers: u32,
) -> Result<(), StoreError> {
    let key = relay_index_key(relay_url, id);
    relay_index
        .put(txn, &key, &[])
        // Grow-path put: classify so an `MDB_MAP_FULL` surfaces as the typed
        // `StoreError::MapFull` health variant (#1521), never a stringly `Io`.
        .map_err(|e| classify_heed_err(e, map_size, max_readers))
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

// ─── #1518 relay×kind presence index ─────────────────────────────────────────

/// Separator byte between the `relay_url` and `kind` segments of a relay-kind
/// key.  Same rationale as `RELAY_INDEX_SEP`: relay URLs are valid UTF-8 and
/// never contain a NUL byte.
const RELAY_KIND_SEP: u8 = 0x00;

// Privacy gate (`is_relay_provenance_private`) is the single source of truth in
// `crate::types::ids` — shared with the in-memory backend so both gate the same
// kinds.

/// Encode a relay-kind key as `relay_url || 0x00 || kind(BE4) || event_id(32)`.
///
/// `relay_url` comes first so all entries for one relay are a contiguous prefix
/// range; `kind` next so one relay's entries for a single kind are a contiguous
/// sub-range (big-endian so the byte order matches numeric order).
fn relay_kind_key(relay_url: &str, kind: u32, id: &EventId) -> Vec<u8> {
    let mut k = Vec::with_capacity(relay_url.len() + 1 + 4 + 32);
    k.extend_from_slice(relay_url.as_bytes());
    k.push(RELAY_KIND_SEP);
    k.extend_from_slice(&kind.to_be_bytes());
    k.extend_from_slice(id);
    k
}

/// Inclusive lower bound for a prefix scan of one relay's entries.
pub(super) fn relay_kind_relay_lo(relay_url: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(relay_url.len() + 1);
    k.extend_from_slice(relay_url.as_bytes());
    k.push(RELAY_KIND_SEP);
    k
}

/// Exclusive upper bound for a prefix scan of one relay's entries.
pub(super) fn relay_kind_relay_hi(relay_url: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(relay_url.len() + 1);
    k.extend_from_slice(relay_url.as_bytes());
    k.push(RELAY_KIND_SEP + 1); // 0x01
    k
}

/// Inclusive lower bound for a scan of one relay's entries for a single kind.
pub(super) fn relay_kind_kind_lo(relay_url: &str, kind: u32) -> Vec<u8> {
    let mut k = Vec::with_capacity(relay_url.len() + 1 + 4);
    k.extend_from_slice(relay_url.as_bytes());
    k.push(RELAY_KIND_SEP);
    k.extend_from_slice(&kind.to_be_bytes());
    k
}

/// Exclusive upper bound for a scan of one relay's entries for a single kind.
///
/// For `kind == u32::MAX` there is no `kind + 1`, so we fall back to the relay's
/// upper bound (`relay_url || 0x01`), which is still exclusive of every entry
/// for that relay/kind.
pub(super) fn relay_kind_kind_hi(relay_url: &str, kind: u32) -> Vec<u8> {
    if kind == u32::MAX {
        return relay_kind_relay_hi(relay_url);
    }
    let mut k = Vec::with_capacity(relay_url.len() + 1 + 4);
    k.extend_from_slice(relay_url.as_bytes());
    k.push(RELAY_KIND_SEP);
    k.extend_from_slice(&(kind + 1).to_be_bytes());
    k
}

/// Decode the `kind` segment of a relay-kind key for a relay whose URL byte
/// length is `relay_url_len`.  Returns `None` if the key is malformed.
pub(super) fn relay_kind_kind_from_key(key: &[u8], relay_url_len: usize) -> Option<u32> {
    // Expected layout: relay_url_len + 1 separator + 4 kind + 32 id bytes.
    if key.len() != relay_url_len + 1 + 4 + 32 {
        return None;
    }
    let start = relay_url_len + 1;
    let kind_bytes: [u8; 4] = key[start..start + 4].try_into().ok()?;
    Some(u32::from_be_bytes(kind_bytes))
}

/// Write a `(relay_url, kind, event_id)` presence entry — privacy-gated.
///
/// A no-op for `is_relay_provenance_private(kind)` so a privacy-gated kind can
/// never enter the index regardless of which write path calls this.
pub(super) fn relay_kind_put(
    relay_kind: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    relay_url: &str,
    kind: u32,
    id: &EventId,
    map_size: usize,
    max_readers: u32,
) -> Result<(), StoreError> {
    if crate::types::is_relay_provenance_private(kind) {
        return Ok(());
    }
    let key = relay_kind_key(relay_url, kind, id);
    relay_kind
        .put(txn, &key, &[])
        // Grow-path put: classify so an `MDB_MAP_FULL` surfaces as the typed
        // `StoreError::MapFull` health variant (#1521), never a stringly `Io`.
        .map_err(|e| classify_heed_err(e, map_size, max_readers))
}

/// Remove a single `(relay_url, kind, event_id)` entry from the relay-kind index.
pub(super) fn relay_kind_delete_exact(
    relay_kind: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    relay_url: &str,
    kind: u32,
    id: &EventId,
) -> Result<(), StoreError> {
    let key = relay_kind_key(relay_url, kind, id);
    relay_kind
        .delete(txn, &key)
        .map_err(|e| StoreError::Io(format!("relay_kind delete: {e}")))?;
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
#[allow(clippy::too_many_arguments)]
pub(super) fn upsert(
    db: Database<Bytes, Bytes>,
    relay_index: Database<Bytes, Bytes>,
    relay_kind: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    id: &EventId,
    relay_url: RelayUrl,
    kind: u32,
    received_at_ms: u64,
    map_size: usize,
    max_readers: u32,
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
            .map_err(|e| classify_heed_err(e, map_size, max_readers))?;
        // Index entry already present (idempotent), but re-assert for safety.
        relay_index_put(relay_index, txn, &relay_url, id, map_size, max_readers)?;
        relay_kind_put(relay_kind, txn, &relay_url, kind, id, map_size, max_readers)?;
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
                .map_err(|e| classify_heed_err(e, map_size, max_readers))?;
            // Index: drop the evicted relay, add the incoming one.  Only drop the
            // evicted entry if no surviving provenance entry still references it
            // (it cannot, since provenance relays are unique, but stay defensive).
            if !entries.iter().any(|e| e.relay_url == evicted_relay) {
                relay_index_delete_exact(relay_index, txn, &evicted_relay, id)?;
                relay_kind_delete_exact(relay_kind, txn, &evicted_relay, kind, id)?;
            }
            relay_index_put(relay_index, txn, &relay_url, id, map_size, max_readers)?;
            relay_kind_put(relay_kind, txn, &relay_url, kind, id, map_size, max_readers)?;
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
        .map_err(|e| classify_heed_err(e, map_size, max_readers))?;
    relay_index_put(relay_index, txn, &relay_url, id, map_size, max_readers)?;
    relay_kind_put(relay_kind, txn, &relay_url, kind, id, map_size, max_readers)?;
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
    relay_kind: Database<Bytes, Bytes>,
    txn: &mut RwTxn,
    id: &EventId,
    kind: u32,
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
        // #1518: also drop the relay×kind entry. A no-op key for a privacy-gated
        // kind (never written) — delete tolerates a missing key.
        relay_kind_delete_exact(relay_kind, txn, &relay_url, kind, id)?;
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
