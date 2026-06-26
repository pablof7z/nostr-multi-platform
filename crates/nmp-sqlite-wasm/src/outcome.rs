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
