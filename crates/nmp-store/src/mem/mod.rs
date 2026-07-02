//! In-memory `EventStore` backend.
//!
//! Used for tests and the pre-M15 web target. Every method is fully implemented
//! against a `Mutex<MemState>` so tests cover the same logic that the LMDB
//! backend will eventually call.
//!
//! See `docs/design/lmdb/trait.md` §5 ("Two backends in v1").
//!
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! # ⚠️  PERFORMANCE WARNING — TESTS ONLY, NOT FOR PRODUCTION  ⚠️
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//!
//! **This backend has NO indexes. Every query is an O(N) full-table scan.**
//!
//! - All six `scan_by_*` functions in [`query`] iterate the *entire* event map,
//!   then perform an O(N log N) sort — regardless of the requested `limit`.
//! - Replaceable-event supersession ([`insert::insert`] →
//!   `handle_supersession`) is **O(N) per insert**: each write linearly scans
//!   every stored event to find the row it would replace.
//! - The [`crate::EventStore`] trait advertises named indexes; this
//!   backend implements *none* of them. It only fakes the contract by scanning.
//!
//! This is **fine for tests and small WASM builds** (small N, the intended use
//! cases). It is **catastrophic in production**: with thousands of events,
//! reads and writes degrade quadratically and you hit a hard performance cliff
//! with no warning.
//!
//! ## For production, use the LMDB backend instead
//!
//! Enable the `lmdb-backend` feature and use the `nmp-nostr-lmdb` backend,
//! which has real B-tree indexes for every query path. Do **not** wire
//! `MemEventStore` into a production relay connection or any long-lived store.
//!
//! ## Tracking
//!
//! This limitation is a known, accepted trade-off. It is documented loudly here
//! by design so future developers do not hit the cliff by accident instead of
//! relying on a parallel planning queue.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//!
//! Module layout:
//!   mod.rs        — factory, `MemState`, `MemEventStore`, provenance helpers
//!   `store_impl.rs` — `EventStore` trait impl (delegation to sub-modules)
//!   insert.rs     — §7.1 insert invariants (replaceable, kind:5, normal)
//!   query.rs      — read / scan methods
//!   gc.rs         — gc_step / LRU eviction / tombstone purge
//!   domain.rs     — domain rows + migrations

pub(super) mod domain;
// #1811 — in-memory full-text inverted index (parity target for the Phase-2
// LMDB FTS sub-databases).
pub(super) mod fts;
pub(super) mod gc;
pub(super) mod insert;
// NIP-09 (kind:5) deletion handler — extracted from insert.rs for the 500 LOC cap.
pub(super) mod ingest_log;
pub(super) mod insert_kind5;
pub(super) mod query;
pub(super) mod query_tags;
mod store_impl;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use super::types::{EventId, ProvenanceEntry, RelayUrl, StoredEvent, TombstoneRow};
use super::StoreError;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Maximum provenance entries kept per event.
pub(super) const MAX_PROVENANCE_ENTRIES: usize = 32;

/// Tombstones older than this many seconds are purged by `gc_step`.
pub(super) const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 3600; // 90 days

// ─── Shared storage type ─────────────────────────────────────────────────────

/// Shared storage map for a single domain namespace.
type DomainMap = Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>;

// ─── Inner state ─────────────────────────────────────────────────────────────

pub(super) struct MemState {
    /// Primary event store: hex id → `StoredEvent`.
    pub(super) events: HashMap<String, StoredEvent>,

    /// Tombstone rows: hex `target_id` → `TombstoneRow`.
    pub(super) tombstones: HashMap<String, TombstoneRow>,

    /// Address tombstones (kind:5 `a`-tag): "kind:pubkey:dtag" → `TombstoneRow`.
    pub(super) addr_tombstones: HashMap<String, TombstoneRow>,

    /// Provenance: hex `event_id` → sorted Vec<ProvenanceEntry>.
    pub(super) provenance: HashMap<String, Vec<ProvenanceEntry>>,

    /// Relay-origin reverse index: relay_url → BTreeSet<hex event_id>.
    ///
    /// Maintained symmetrically with `provenance` — any `insert` that records a
    /// `ProvenanceEntry` also records the (relay_url, event_id) pair here, and any
    /// removal of an event (delete, replaceable supersession, GC) removes the entry.
    ///
    /// This makes `list_events_seen_on(url)` an O(1) lookup instead of an O(N)
    /// full-provenance scan. The set is capped only by the per-relay event population,
    /// which is bounded by the provenance LRU (MAX_PROVENANCE_ENTRIES per event)
    /// and the global event set.
    ///
    /// V-52: used by `EventStore::list_events_seen_on`.
    pub(super) relay_index: HashMap<RelayUrl, BTreeSet<String>>,

    /// #1518 relay×kind presence index: relay_url → kind → BTreeSet<hex event_id>.
    ///
    /// Parity with the LMDB backend's `nmp-relay-kind` sub-db. Maintained
    /// symmetrically with `provenance`/`relay_index` — every insert that records
    /// a `ProvenanceEntry` for a non-private kind also records the
    /// (relay_url, kind, event_id) triple here; every removal prunes it.
    ///
    /// Privacy-gated: NIP-04/17/59 kinds never enter (checked in
    /// `relay_kind_add` via `crate::types::is_relay_provenance_private`).
    ///
    /// Used by `EventStore::relay_kind_coverage` / `relay_kind_count`.
    pub(super) relay_kind: HashMap<RelayUrl, HashMap<u32, BTreeSet<String>>>,

    /// F-TTL replaceable freshness: `ReplaceableKey` → `check_again_after_unix_ms`.
    ///
    /// Parity with the LMDB backend's `replaceable_freshness` sub-db so the
    /// kernel's TTL gate (`claim_replaceable`) behaves identically on the
    /// in-memory backend used by tests and the lmdb-off build.
    pub(super) replaceable_freshness: HashMap<crate::ReplaceableKey, u64>,

    /// K3 coverage ledger (ADR-0072 §3, Stage D1): `(filter_hash, relay)` →
    /// `covered_through` unix-seconds. Parity with the LMDB backend's
    /// `nmp-coverage` sub-db. Downward-closed and monotonic — see
    /// `EventStore::record_coverage` / `crate::CoverageRow`.
    pub(super) coverage: HashMap<(String, String), u64>,

    /// Domain data per namespace.
    pub(super) domain_data: HashMap<&'static str, DomainMap>,

    /// Domain schema versions.
    pub(super) domain_versions: HashMap<&'static str, u32>,

    // ─── Ingest log (ADR-0072 §3) ───────────────────────────────────────────────
    /// Monotonic ingest sequence counter. Starts at 0 (no entries); first real seq is 1.
    /// D4: incremented only inside the MemState mutex.
    pub(super) ingest_seq: u64,

    /// In-memory ingest log: seq → StoreLogEntry. Bounded to DEFAULT_LOG_MAX_ENTRIES.
    pub(super) ingest_log: std::collections::BTreeMap<u64, crate::ingest_log::StoreLogEntry>,

    /// GC floor: the lowest seq that has been trimmed. Entries ≤ this value are gone.
    pub(super) log_gc_floor: u64,

    /// VOLATILE `Protected`-cursor log-retention claims (ADR-0072 §6, step-4).
    ///
    /// Held under the SAME mutex as `ingest_log` / `log_gc_floor` / `ingest_seq`,
    /// so the append-time trim reads a consistent claim set within the same
    /// locked write the seq was allocated in. Written wholesale by
    /// `EventStore::replace_log_retention_claims` (kernel = single writer);
    /// never persisted.
    pub(super) retention_claims: Vec<crate::ingest_log::LogRetentionClaim>,

    // ─── LRU access tracking (V-60) ──────────────────────────────────────────
    /// Monotonically-increasing counter.  Incremented by one on every insert
    /// and every point-read (get_by_id).  Using a counter rather than wall-clock
    /// ms avoids a D7 surface on the read path while still producing a strict
    /// total order for LRU eviction (no ties possible).
    pub(super) access_seq: u64,

    /// LRU access index: hex event_id → last-access sequence number.
    ///
    /// Set to `access_seq` on insert and bumped on every `get_by_id` that
    /// returns `Some`.  Removed symmetrically whenever an event is evicted or
    /// deleted from the primary map.
    pub(super) access_index: HashMap<String, u64>,

    /// #1811 — full-text inverted index (installed specs + per-scope index).
    ///
    /// Empty until `install_search_index_specs` runs at composition. Maintained
    /// symmetrically with the event map: every insert/replace/delete/GC site
    /// calls `fts::fts_index_add` / `fts::fts_index_remove`, so a search hit
    /// never survives source deletion. Parity target for the Phase-2 LMDB FTS
    /// sub-databases.
    pub(in crate::mem) fts: fts::FtsState,
}

impl MemState {
    pub(super) fn new() -> Self {
        Self {
            events: HashMap::new(),
            tombstones: HashMap::new(),
            addr_tombstones: HashMap::new(),
            provenance: HashMap::new(),
            relay_index: HashMap::new(),
            relay_kind: HashMap::new(),
            replaceable_freshness: HashMap::new(),
            coverage: HashMap::new(),
            domain_data: HashMap::new(),
            domain_versions: HashMap::new(),
            access_seq: 0,
            access_index: HashMap::new(),
            ingest_seq: 0,
            ingest_log: std::collections::BTreeMap::new(),
            log_gc_floor: 0,
            retention_claims: Vec::new(),
            fts: fts::FtsState::default(),
        }
    }
}

// ─── MemEventStore ───────────────────────────────────────────────────────────

/// Fully in-memory `EventStore` implementation.
pub struct MemEventStore {
    pub(super) state: Mutex<MemState>,
}

impl MemEventStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(MemState::new()),
        }
    }

    pub(super) fn lock(&self) -> Result<std::sync::MutexGuard<'_, MemState>, StoreError> {
        self.state.lock().map_err(|e| StoreError::Io(e.to_string()))
    }
}

impl Default for MemEventStore {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Provenance helpers ──────────────────────────────────────────────────────

pub(super) fn sort_provenance(entries: &mut [ProvenanceEntry]) {
    entries.sort_by(|a, b| {
        a.first_seen_ms
            .cmp(&b.first_seen_ms)
            .then(a.relay_url.cmp(&b.relay_url))
    });
    for (i, e) in entries.iter_mut().enumerate() {
        e.primary = i == 0;
    }
}

pub(super) fn upsert_provenance(
    entries: &mut Vec<ProvenanceEntry>,
    relay_url: RelayUrl,
    received_at_ms: u64,
) {
    // Update existing entry if present.
    if let Some(e) = entries.iter_mut().find(|e| e.relay_url == relay_url) {
        if received_at_ms < e.first_seen_ms {
            e.first_seen_ms = received_at_ms;
        }
        if received_at_ms > e.last_seen_ms {
            e.last_seen_ms = received_at_ms;
        }
        sort_provenance(entries);
        return;
    }

    // If at capacity, overwrite the oldest non-primary entry.
    if entries.len() >= MAX_PROVENANCE_ENTRIES {
        if let Some(oldest) = entries.iter_mut().skip(1).min_by_key(|e| e.last_seen_ms) {
            *oldest = ProvenanceEntry {
                relay_url,
                first_seen_ms: received_at_ms,
                last_seen_ms: received_at_ms,
                primary: false,
            };
            sort_provenance(entries);
            return;
        }
    }

    entries.push(ProvenanceEntry {
        relay_url,
        first_seen_ms: received_at_ms,
        last_seen_ms: received_at_ms,
        primary: false,
    });
    sort_provenance(entries);
}

// ─── Relay index helpers (V-52) ──────────────────────────────────────────────

/// Add (relay_url, event_id_hex) to the relay reverse index.
///
/// Idempotent — inserting the same pair twice is a no-op (BTreeSet semantics).
pub(super) fn relay_index_add(st: &mut MemState, relay_url: &RelayUrl, id_hex: &str) {
    st.relay_index
        .entry(relay_url.clone())
        .or_default()
        .insert(id_hex.to_string());
}

/// Remove event_id_hex from every relay entry in the relay reverse index.
///
/// Called when an event is removed (delete, supersession, GC expiry) so the
/// index never contains dangling references. If removing the id leaves a relay
/// entry empty, the entry itself is dropped (avoids unbounded map growth).
pub(super) fn relay_index_remove(st: &mut MemState, id_hex: &str) {
    let empty_relays: Vec<RelayUrl> = st
        .relay_index
        .iter_mut()
        .filter_map(|(url, ids)| {
            ids.remove(id_hex);
            if ids.is_empty() {
                Some(url.clone())
            } else {
                None
            }
        })
        .collect();
    for url in empty_relays {
        st.relay_index.remove(&url);
    }
}

/// List event ids seen on `relay_url` — O(1) lookup.
///
/// Returns a sorted vec of hex event id strings. Events that have been removed
/// from the store are not included — the index stays consistent with the primary
/// event map because every removal path calls `relay_index_remove`.
///
/// The EventId ([u8;32]) return type requires a hex → bytes conversion for each
/// id.  The call frequency (browse-view rendering, test assertions) is low
/// enough that the per-call allocation is acceptable (D8: no hot-path concern
/// here; this is not the 4 Hz snapshot tick).
pub(super) fn list_seen_on(st: &MemState, relay_url: &str) -> Vec<EventId> {
    let Some(ids) = st.relay_index.get(relay_url) else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(|hex| super::types::hex_to_event_id(hex))
        .collect()
}

// ─── Relay×kind index helpers (#1518) ────────────────────────────────────────

/// Add (relay_url, kind, event_id_hex) to the relay×kind index — privacy-gated.
///
/// A no-op for `is_relay_provenance_private(kind)` so a privacy-gated kind can
/// never enter the index regardless of which insert path calls this. Idempotent
/// (BTreeSet semantics).
pub(super) fn relay_kind_add(st: &mut MemState, relay_url: &RelayUrl, kind: u32, id_hex: &str) {
    if super::types::is_relay_provenance_private(kind) {
        return;
    }
    st.relay_kind
        .entry(relay_url.clone())
        .or_default()
        .entry(kind)
        .or_default()
        .insert(id_hex.to_string());
}

/// Remove event_id_hex from every (relay, kind) entry in the relay×kind index.
///
/// Mirrors `relay_index_remove`: called on every event removal so the index
/// never carries a dangling reference. Empty kind- and relay-buckets are pruned
/// to bound map growth.
pub(super) fn relay_kind_remove_id(st: &mut MemState, id_hex: &str) {
    let mut empty_relays: Vec<RelayUrl> = Vec::new();
    for (url, kinds) in st.relay_kind.iter_mut() {
        let mut empty_kinds: Vec<u32> = Vec::new();
        for (kind, ids) in kinds.iter_mut() {
            ids.remove(id_hex);
            if ids.is_empty() {
                empty_kinds.push(*kind);
            }
        }
        for k in empty_kinds {
            kinds.remove(&k);
        }
        if kinds.is_empty() {
            empty_relays.push(url.clone());
        }
    }
    for url in empty_relays {
        st.relay_kind.remove(&url);
    }
}

/// Distinct kinds seen on `relay_url`, ascending.
pub(super) fn relay_kind_coverage(st: &MemState, relay_url: &str) -> Vec<u32> {
    let Some(kinds) = st.relay_kind.get(relay_url) else {
        return Vec::new();
    };
    let mut out: Vec<u32> = kinds
        .keys()
        .copied()
        .filter(|kind| !super::types::is_relay_provenance_private(*kind))
        .collect();
    out.sort_unstable();
    out
}

/// Count of distinct events of `kind` seen on `relay_url`.
pub(super) fn relay_kind_count(st: &MemState, relay_url: &str, kind: u32) -> u64 {
    if super::types::is_relay_provenance_private(kind) {
        return 0;
    }
    st.relay_kind
        .get(relay_url)
        .and_then(|kinds| kinds.get(&kind))
        .map(|ids| ids.len() as u64)
        .unwrap_or(0)
}

// ─── LRU access helpers (V-60) ───────────────────────────────────────────────

/// Record an access on `id_hex`.  Increments `access_seq` and stores the new
/// value in `access_index`.  Call on insert and on every `get_by_id` hit.
pub(super) fn access_stamp(st: &mut MemState, id_hex: &str) {
    st.access_seq += 1;
    st.access_index.insert(id_hex.to_string(), st.access_seq);
}

/// Remove LRU tracking entry for `id_hex`.
/// Call whenever an event is removed from the primary `events` map.
pub(super) fn access_remove(st: &mut MemState, id_hex: &str) {
    st.access_index.remove(id_hex);
}

// ─── Hex utilities ───────────────────────────────────────────────────────────

pub(super) fn bytes_to_hex(b: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(b.len() * 2);
    for byte in b {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
