//! Insert outcomes, reject reasons, tombstones, and provenance types.
//!
//! D2: store returns typed outcomes, never panics or `unwrap`s on bad input.

use serde::{Deserialize, Serialize};
use super::ids::{EventId, PubKey, RelayUrl};

// ─── Insert outcomes ─────────────────────────────────────────────────────────

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

// ─── Tombstones ──────────────────────────────────────────────────────────────

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TombstoneOrigin {
    Kind5,
    NIP40Expiry,
    AdminPurge,
}

// ─── Provenance ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ProvenanceEntry {
    pub relay_url: RelayUrl,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    /// True for the first relay that delivered this event (deterministic after sort).
    pub primary: bool,
}
