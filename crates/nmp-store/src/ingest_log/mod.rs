//! Ingest-log types for the pull-based event-log consumption system.
//!
//! ADR-0058 §3 — the primitive. Consumed by `EventStore::scan_log_since_seq`,
//! `EventStore::latest_ingest_seq`, and `EventStore::oldest_available_seq`.

use crate::types::{EventId, RawEvent, RelayUrl};

/// Default maximum ingest-log entries retained per backend (ADR-0058 R2.4).
pub const DEFAULT_LOG_MAX_ENTRIES: u64 = 10_000;

/// Why an event was semantically removed (ADR-0058 §3 Rev 3).
///
/// LRU eviction and `delete_by_filter(ByRelayOnly)` emit NO row — those are
/// retention removals, not semantic deletes.
///
/// DURABLE on-disk format: variant names are pinned via `#[serde(rename)]`.
/// A future Rust rename MUST NOT change the string — change the rename value
/// instead. (SHOULD-FIX 6: stable persisted schema.)
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeleteReason {
    /// NIP-09 author-signed kind:5 delete.
    #[serde(rename = "Nip09")]
    Nip09,
    /// NIP-40 expiration timestamp reached (store-side reap).
    #[serde(rename = "Nip40Expiry")]
    Nip40Expiry,
    /// Operator / admin purge via `delete_by_filter(ByAuthor|ByIds|ByKindRange)`.
    #[serde(rename = "AdminPurge")]
    AdminPurge,
}

/// The operation recorded in a `StoreLogEntry`.
///
/// DURABLE on-disk format: variant and field names are pinned via `#[serde(rename)]`.
/// A future Rust rename MUST NOT change the string. (SHOULD-FIX 6.)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LogOp {
    /// A new distinct event was accepted and stored.
    #[serde(rename = "Inserted")]
    Inserted,
    /// A replaceable event superseded an older one.
    #[serde(rename = "Replaced")]
    Replaced {
        /// The event id that was overwritten.
        #[serde(rename = "replaced_id")]
        replaced_id: EventId,
    },
    /// An event was semantically removed.
    #[serde(rename = "Deleted")]
    Deleted {
        /// The event id that was removed.
        #[serde(rename = "target_id")]
        target_id: EventId,
        /// The reason for removal.
        #[serde(rename = "reason")]
        reason: DeleteReason,
    },
}

/// One entry in the store's ingest log (ADR-0058 §3).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoreLogEntry {
    /// Monotonic ingest-order sequence number. Never reused, starts at 1.
    pub seq: u64,
    /// What happened.
    pub op: LogOp,
    /// For Inserted/Replaced: the new event id. For Deleted: the kind:5 event id
    /// (use `op` to get the removed event id).
    pub event_id: EventId,
    /// Raw event bytes — `Some` for `Inserted`/`Replaced`, `None` for `Deleted`.
    pub raw_event: Option<RawEvent>,
    /// The relay that delivered this event, or `None` for store-internal ops.
    pub source_relay: Option<RelayUrl>,
    /// Wall-clock milliseconds when the event arrived at this store node.
    pub received_at_ms: u64,
}

/// A page of log entries returned by `scan_log_since_seq` (ADR-0058 §3 R2.1).
///
/// Level-triggered: if `has_more` is true the consumer MUST drain before
/// yielding to the wake loop.
#[derive(Debug, Clone)]
pub struct PullPage {
    /// Entries in ascending `seq` order.
    pub entries: Vec<StoreLogEntry>,
    /// Pass as `after_seq` on the next call.
    pub next_after_seq: u64,
    /// Store's `latest_ingest_seq` at read time.
    pub latest_seq: u64,
    /// True when `next_after_seq < latest_seq`.
    pub has_more: bool,
}

/// Explicit gap — returned when `after_seq` is behind the GC floor.
/// Never a silent skip (ADR-0058 §6 `GapAllowed` contract).
#[derive(Debug, Clone)]
pub struct PullGap {
    /// The `after_seq` the caller requested.
    pub requested_after_seq: u64,
    /// Lowest seq still available; reset the cursor to this value.
    pub first_available_seq: u64,
}

/// Return type of `EventStore::scan_log_since_seq`.
#[derive(Debug, Clone)]
pub enum ScanLogResult {
    /// Normal (possibly partial) page.
    Page(PullPage),
    /// Requested position was GC'd — explicit gap, never a silent skip.
    Gap(PullGap),
}
