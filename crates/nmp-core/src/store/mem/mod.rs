//! In-memory `EventStore` backend.
//!
//! Used for tests and the pre-M15 web target. Every method is fully implemented
//! against a `Mutex<MemState>` so tests cover the same logic that the LMDB
//! backend will eventually call.
//!
//! See `docs/design/lmdb/trait.md` §5 ("Two backends in v1").
//!
//! Module layout (Article I — each sub-module ≤ 300 LOC):
//!   mod.rs      — factory, `MemState`, `MemEventStore`, provenance helpers
//!   store_impl.rs — `EventStore` trait impl (delegation to sub-modules)
//!   insert.rs   — §7.1 insert invariants (replaceable, kind:5, normal)
//!   query.rs    — read / scan methods
//!   gc.rs       — claim / release / prune
//!   domain.rs   — domain rows + migrations

pub(super) mod domain;
pub(super) mod gc;
pub(super) mod insert;
pub(super) mod query;
mod store_impl;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use super::types::{
    ClaimerId, ProvenanceEntry, RelayUrl, StoredEvent, TombstoneRow, WatermarkRow,
};
use super::StoreError;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Default maximum pinned events per view (D8 / gc.md §2).
pub(super) const DEFAULT_VIEW_CEILING: usize = 1_000;

/// Hard global pinned ceiling (D8 / gc.md §2).
pub(super) const MAX_PINNED_TOTAL: usize = 20_000;

/// Maximum provenance entries kept per event.
pub(super) const MAX_PROVENANCE_ENTRIES: usize = 32;

/// Tombstones older than this many seconds are purged by `gc_step`.
pub(super) const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 3600; // 90 days

// ─── Shared storage type ─────────────────────────────────────────────────────

/// Shared storage map for a single domain namespace.
type DomainMap = Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>;

// ─── Inner state ─────────────────────────────────────────────────────────────

pub(super) struct MemState {
    /// Primary event store: hex id → StoredEvent.
    pub(super) events: HashMap<String, StoredEvent>,

    /// Tombstone rows: hex target_id → TombstoneRow.
    pub(super) tombstones: HashMap<String, TombstoneRow>,

    /// Address tombstones (kind:5 `a`-tag): "kind:pubkey:dtag" → TombstoneRow.
    pub(super) addr_tombstones: HashMap<String, TombstoneRow>,

    /// Provenance: hex event_id → sorted Vec<ProvenanceEntry>.
    pub(super) provenance: HashMap<String, Vec<ProvenanceEntry>>,

    /// Watermarks: (filter_hash_hex, relay_url) → WatermarkRow.
    pub(super) watermarks: HashMap<(String, String), WatermarkRow>,

    /// Domain data per namespace.
    pub(super) domain_data: HashMap<&'static str, DomainMap>,

    /// Domain schema versions.
    pub(super) domain_versions: HashMap<&'static str, u32>,

    /// Claim budgets: claimer → max pinned.
    pub(super) claim_budgets: HashMap<ClaimerId, usize>,

    /// Current claims: claimer → BTreeSet of hex event ids.
    /// BTreeSet gives idempotency per T25 — re-claiming a known id is a no-op.
    pub(super) claims: HashMap<ClaimerId, BTreeSet<String>>,
}

impl MemState {
    pub(super) fn new() -> Self {
        Self {
            events: HashMap::new(),
            tombstones: HashMap::new(),
            addr_tombstones: HashMap::new(),
            provenance: HashMap::new(),
            watermarks: HashMap::new(),
            domain_data: HashMap::new(),
            domain_versions: HashMap::new(),
            claim_budgets: HashMap::new(),
            claims: HashMap::new(),
        }
    }

    #[allow(dead_code)] // Available for future dump/debug helpers.
    pub(super) fn events_sorted_newest_first(&self) -> Vec<&StoredEvent> {
        let mut v: Vec<&StoredEvent> = self.events.values().collect();
        v.sort_by(|a, b| {
            b.raw.created_at
                .cmp(&a.raw.created_at)
                .then(a.raw.id.cmp(&b.raw.id))
        });
        v
    }
}

// ─── MemEventStore ───────────────────────────────────────────────────────────

/// Fully in-memory `EventStore` implementation.
pub struct MemEventStore {
    pub(super) state: Mutex<MemState>,
}

impl MemEventStore {
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

// ─── Hex utilities ───────────────────────────────────────────────────────────

pub(super) fn bytes_to_hex(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{byte:02x}")).collect()
}
