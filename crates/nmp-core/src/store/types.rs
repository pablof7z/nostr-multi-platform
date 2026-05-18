//! Supporting types for `EventStore`.
//!
//! These types live here and are re-exported from `nmp_core::store`.
//! They track the design in `docs/design/lmdb/trait/types.md`.
//!
//! NOTE: The design references `nostr::Event` / `nostr::Keys` from the upstream nostr crate.
//! Since that crate is not yet in the workspace, this module uses `RawEvent` as a temporary
//! stand-in. Full signature verification is deferred to the M3-lmdb follow-up task.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── Type aliases ────────────────────────────────────────────────────────────

pub type EventId = [u8; 32];
pub type PubKey = [u8; 32];
pub type RelayUrl = String;

// ─── RawEvent (stand-in for nostr::Event) ────────────────────────────────────

/// Temporary stand-in for `nostr::Event` until the nostr crate is in the workspace.
///
/// Fields match the NIP-01 event object exactly. Signature verification is
/// skipped for now (insert always trusts the caller). The M3-lmdb task will
/// swap this for the real type and enable proper sig checks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawEvent {
    pub id: String,          // lowercase hex
    pub pubkey: String,      // lowercase hex
    pub created_at: u64,     // unix seconds
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,         // lowercase hex
}

impl RawEvent {
    /// Decode hex id → 32 bytes. Returns zeroes on malformed input.
    pub fn id_bytes(&self) -> EventId {
        hex_to_bytes32(&self.id)
    }

    /// Decode hex pubkey → 32 bytes. Returns zeroes on malformed input.
    pub fn pubkey_bytes(&self) -> PubKey {
        hex_to_bytes32(&self.pubkey)
    }

    /// NIP-01 replaceable kinds: 0, 3, and 10000–19999.
    pub fn is_replaceable(&self) -> bool {
        self.kind == 0 || self.kind == 3 || (10_000..20_000).contains(&self.kind)
    }

    /// NIP-33 parameterized replaceable kinds: 30000–39999.
    pub fn is_param_replaceable(&self) -> bool {
        (30_000..40_000).contains(&self.kind)
    }

    /// NIP-16 ephemeral kinds: 20000–29999.
    pub fn is_ephemeral(&self) -> bool {
        (20_000..30_000).contains(&self.kind)
    }

    /// Returns the value of the first `d` tag, if present.
    pub fn d_tag(&self) -> Option<Vec<u8>> {
        self.tags.iter().find(|t| t.first().map(|s| s == "d").unwrap_or(false))
            .and_then(|t| t.get(1))
            .map(|s| s.as_bytes().to_vec())
    }

    /// Returns the unix-second value of the first `expiration` tag, if present.
    pub fn expiration(&self) -> Option<u64> {
        self.tags.iter()
            .find(|t| t.first().map(|s| s == "expiration").unwrap_or(false))
            .and_then(|t| t.get(1))
            .and_then(|s| s.parse::<u64>().ok())
    }

    /// Returns all `e`-tag target ids (lowercase hex).
    pub fn e_tags(&self) -> Vec<String> {
        self.tags.iter()
            .filter(|t| t.first().map(|s| s == "e").unwrap_or(false))
            .filter_map(|t| t.get(1).cloned())
            .collect()
    }

    /// Returns all `p`-tag target pubkeys (lowercase hex).
    pub fn p_tags(&self) -> Vec<String> {
        self.tags.iter()
            .filter(|t| t.first().map(|s| s == "p").unwrap_or(false))
            .filter_map(|t| t.get(1).cloned())
            .collect()
    }

    /// Returns all `a`-tag target addresses (e.g. "30023:pubkey:dtag").
    pub fn a_tags(&self) -> Vec<String> {
        self.tags.iter()
            .filter(|t| t.first().map(|s| s == "a").unwrap_or(false))
            .filter_map(|t| t.get(1).cloned())
            .collect()
    }

    /// Validates the event has a plausible structure (non-empty id, pubkey, sig).
    /// Full cryptographic verification is deferred until the nostr crate is wired in.
    pub fn is_structurally_valid(&self) -> bool {
        self.id.len() == 64 && self.pubkey.len() == 64 && self.sig.len() == 128
    }
}

fn hex_to_bytes32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    if s.len() != 64 {
        return out;
    }
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        if i >= 32 { break; }
        if let (Some(&hi), Some(&lo)) = (chunk.first(), chunk.get(1)) {
            out[i] = (hex_nibble(hi) << 4) | hex_nibble(lo);
        }
    }
    out
}

fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

// ─── StoredEvent ──────────────────────────────────────────────────────────────

/// A stored Nostr event with arrival metadata.
///
/// `raw` is `Arc<RawEvent>` so the hot LRU can hold reference-counted copies
/// without cloning the event body on each `get_by_id`.
#[derive(Clone, Debug)]
pub struct StoredEvent {
    pub raw: Arc<RawEvent>,
    pub received_at_ms: u64,   // wall-clock first arrival across all relays
}

// ─── Insert outcomes ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum InsertOutcome {
    /// Fresh insert; secondary indexes written.
    Inserted { id: EventId, sources_after: u32 },
    /// Duplicate id; provenance updated, primary untouched.
    Duplicate { id: EventId, sources_after: u32 },
    /// Replaceable supersession: this event replaced an older one.
    Replaced { new_id: EventId, replaced_id: EventId },
    /// Replaceable supersession: incoming was older, dropped.
    Superseded { id: EventId, current_id: EventId },
    /// Suppressed because a tombstone exists for this event id.
    Tombstoned { id: EventId, kind5_event_id: Option<EventId>, origin: TombstoneOrigin },
    /// Signature / delegation / structural validity failed.
    Rejected { id: EventId, reason: RejectReason },
    /// Ephemeral kind: delivered to live consumers, not stored.
    Ephemeral { id: EventId },
}

#[derive(Clone, Debug)]
pub enum RejectReason {
    BadSignature,
    BadDelegation(String),
    Malformed(String),
    /// NIP-40 expiration already in the past.
    ExpiredOnArrival,
}

// ─── Tombstones ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct TombstoneRow {
    pub target_id: EventId,
    /// None for NIP-40 expiry and AdminPurge tombstones.
    pub kind5_event_id: Option<EventId>,
    /// None for NIP40Expiry / AdminPurge.
    pub deleter_pubkey: Option<PubKey>,
    /// Unix seconds; max observed across redeliveries.
    pub deleted_at: u64,
    pub sources: Vec<RelayUrl>,
    pub origin: TombstoneOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TombstoneOrigin {
    Kind5,
    NIP40Expiry,
    AdminPurge,
}

// ─── Provenance ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ProvenanceEntry {
    pub relay_url: RelayUrl,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    /// True for the first relay that delivered this event (deterministic after sort).
    pub primary: bool,
}

// ─── Watermarks ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct WatermarkKey {
    pub filter_hash: [u8; 32],
    pub relay_url: RelayUrl,
}

#[derive(Clone, Debug)]
pub struct WatermarkRow {
    pub key: WatermarkKey,
    pub synced_up_to: u64,    // unix seconds
    pub last_sync_method: SyncMethod,
    /// Engine-opaque resume blob (M4).
    pub last_negentropy_state: Option<Vec<u8>>,
    pub bytes_saved_vs_req: u64,
    pub updated_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncMethod {
    Negentropy,
    ReqScan,
    Manual,
}

/// Returned by `coverage()` to classify watermark freshness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Coverage {
    /// Fully synced; a cache miss is authoritative "doesn't exist".
    CompleteAsOf(u64),
    /// Synced up to timestamp but row is stale — fetch is needed.
    PartialUpTo(u64),
    /// No watermark; always fetch.
    Unknown,
}

// ─── GC / hot-set ─────────────────────────────────────────────────────────────

/// Opaque view-handle id assigned by the actor (monotonically increasing u64).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ClaimerId(pub u64);

/// Budget for one `gc_step()` call.
#[derive(Clone, Copy, Debug)]
pub struct GcBudget {
    pub max_events_per_step: usize,
    pub max_duration_ms: u32,
}

/// Report produced by `gc_step()`.
#[derive(Clone, Debug, Default)]
pub struct GcReport {
    pub expired_reaped: usize,
    pub lru_evicted: usize,
    pub tombstones_purged: usize,
    pub duration_ms: u32,
}

// ─── Filters ──────────────────────────────────────────────────────────────────

/// NMP-internal delete filter — NOT a pass-through to nostr::Filter.
/// Only exposes operations the kernel legitimately needs; does not allow
/// arbitrary remote filters as a delete vector.
#[derive(Clone, Debug)]
pub enum DeleteFilter {
    /// All events sourced exclusively from this relay.
    ByRelayOnly(RelayUrl),
    /// All events by a specific pubkey.
    ByAuthor(PubKey),
    /// Specific event ids.
    ByIds(Vec<EventId>),
    /// All events with kind in `[lo, hi]` (inclusive range).
    ByKindRange { lo: u32, hi: u32 },
}

// ─── Export ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub enum DumpFormat {
    Jsonl,
    Cbor,
}

#[derive(Clone, Debug, Default)]
pub struct DumpStats {
    pub events: u64,
    pub tombstones: u64,
    pub watermarks: u64,
    pub domain_rows: u64,
    pub bytes_written: u64,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum StoreError {
    Io(String),
    Corrupt(String),
    Encoding(String),
    SchemaTooNew {
        namespace: String,
        on_disk: u32,
        expected: u32,
    },
    MigrationFailed {
        namespace: String,
        from: u32,
        to: u32,
        reason: String,
    },
    UnknownNamespace(String),
    /// Returned by `claim()` when the per-view or global pinned ceiling is exceeded.
    OverPinned {
        claimer: ClaimerId,
        requested: usize,
        ceiling: usize,
    },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(s) => write!(f, "backend i/o: {s}"),
            StoreError::Corrupt(s) => write!(f, "backend corruption: {s}"),
            StoreError::Encoding(s) => write!(f, "encoding: {s}"),
            StoreError::SchemaTooNew { namespace, on_disk, expected } =>
                write!(f, "schema too new: {namespace} on-disk={on_disk} expected={expected}"),
            StoreError::MigrationFailed { namespace, from, to, reason } =>
                write!(f, "schema migration failed: {namespace} v{from}->v{to}: {reason}"),
            StoreError::UnknownNamespace(s) => write!(f, "unknown namespace: {s}"),
            StoreError::OverPinned { claimer, requested, ceiling } =>
                write!(f, "claim ceiling exceeded: claimer={claimer:?} requested={requested} ceiling={ceiling}"),
        }
    }
}

impl std::error::Error for StoreError {}
