//! Crate-local insert-outcome + id types (#1007 PR-3).
//!
//! These mirror `nmp_store::{EventId, PubKey, InsertOutcome, RejectReason,
//! TombstoneOrigin}` field-for-field. The crate cannot depend on `nmp-store`
//! (Cargo cycle), so the `nmp-store` `EventStore` wrapper maps these 1:1 at the
//! cycle-free seam — exactly as it converts `SqliteWasmError -> StoreError`.
//!
//! Pure and target-agnostic.

/// 32-byte event id (mirror of `nmp_store::EventId`).
pub type EventId = [u8; 32];
/// 32-byte public key (mirror of `nmp_store::PubKey`).
pub type PubKey = [u8; 32];

/// Result of an [`crate::OpfsSqliteStore::insert`]. Mirrors
/// `nmp_store::InsertOutcome`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InsertOutcome {
    /// Fresh insert; primary row + secondary indexes + provenance + ingest-log
    /// entry written.
    Inserted {
        /// Id of the stored event.
        id: EventId,
        /// Provenance entry count after the insert.
        sources_after: u32,
    },
    /// Duplicate id; provenance updated, primary untouched.
    Duplicate {
        /// Id of the existing event.
        id: EventId,
        /// Provenance entry count after the upsert.
        sources_after: u32,
    },
    /// Replaceable supersession: this event replaced an older one.
    Replaced {
        /// Id of the newly stored event.
        new_id: EventId,
        /// Id of the event it superseded (now removed).
        replaced_id: EventId,
    },
    /// Replaceable supersession: incoming was older (or lost the id tie-break)
    /// and was dropped.
    Superseded {
        /// Id of the dropped incoming event.
        id: EventId,
        /// Id of the retained current event.
        current_id: EventId,
    },
    /// Suppressed because a tombstone exists for this event id / coordinate.
    Tombstoned {
        /// Id of the suppressed event.
        id: EventId,
        /// The kind:5 event that caused the tombstone, if any.
        kind5_event_id: Option<EventId>,
        /// What kind of tombstone matched.
        origin: TombstoneOrigin,
    },
    /// Structural validity failed (the caller must verify the signature before
    /// insert; this gate only catches malformed wire shape).
    Rejected {
        /// Id of the rejected event (zeroed if the id itself was unparseable).
        id: EventId,
        /// Why it was rejected.
        reason: RejectReason,
    },
    /// Ephemeral kind: never stored.
    Ephemeral {
        /// Id of the ephemeral event.
        id: EventId,
    },
}

/// Why an insert was rejected. Mirrors `nmp_store::RejectReason`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// Malformed wire shape (bad id/pubkey/sig length or non-hex).
    Malformed(String),
    /// NIP-40 expiration already in the past on arrival.
    ExpiredOnArrival,
}

/// What produced a tombstone. Mirrors `nmp_store::TombstoneOrigin`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TombstoneOrigin {
    /// NIP-09 kind:5 deletion.
    Kind5,
    /// NIP-40 expiry.
    NIP40Expiry,
    /// Administrative purge.
    AdminPurge,
}

/// A read-side tombstone row (mirror of `nmp_store::TombstoneRow`).
///
/// Returned by [`crate::OpfsSqliteStore::tombstones_for`] /
/// [`crate::OpfsSqliteStore::list_tombstones`]. The engine's `tombstones` table
/// carries a single optional `source` column, so `sources` holds at most one
/// entry (empty when the row has no recorded source); the `nmp-store` wrapper
/// widens it into the trait's `Vec<RelayUrl>` 1:1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TombstoneRow {
    /// The deleted event id.
    pub target_id: EventId,
    /// The kind:5 event that produced this tombstone (`None` for NIP-40 / admin).
    pub kind5_event_id: Option<EventId>,
    /// The deleter pubkey (`None` for NIP-40 / admin).
    pub deleter_pubkey: Option<PubKey>,
    /// Unix seconds; max observed across redeliveries.
    pub deleted_at: u64,
    /// The source relay(s) — at most one for this engine.
    pub sources: Vec<String>,
    /// What produced the tombstone.
    pub origin: TombstoneOrigin,
}

/// A read-side provenance row (mirror of `nmp_store::ProvenanceEntry`).
///
/// Returned by [`crate::OpfsSqliteStore::provenance_for`], sorted
/// `(first_seen_ms asc, relay_url asc)` so index 0 is the deterministic primary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvenanceRow {
    /// The relay that delivered this copy.
    pub relay_url: String,
    /// First wall-clock arrival from this relay, unix milliseconds.
    pub first_seen_ms: u64,
    /// Most recent wall-clock arrival from this relay, unix milliseconds.
    pub last_seen_ms: u64,
    /// True for the first relay that delivered this event (deterministic).
    pub is_primary: bool,
}
