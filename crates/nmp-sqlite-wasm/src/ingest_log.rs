//! Ingest-journal types for the OPFS-SQLite engine (#1007 PR-3 + PR-5).
//!
//! The entry/op/page types are pure and target-agnostic (they name only
//! [`EngineEvent`] and crate ids), so they compile on native and back the future
//! `nmp-store` wrapper at the cycle-free seam. The SQLite read/write/trim paths
//! that operate on them live in the wasm-gated [`crate::ingest_log_store`]
//! module; this file re-exports its append helpers for `insert`/`delete`.
//!
//! Semantics mirror the LMDB ingest log (ADR-0058 §3-4): every accepted write
//! appends one entry inside the same transaction as the write (D4 — the seq is
//! allocated and committed atomically with the event); the `seq` column is
//! `INTEGER PRIMARY KEY AUTOINCREMENT`, so it is monotonic and never reused even
//! after the append-time trim (BLOCKING 4 — the log is bounded immediately after
//! every append, never only between GC passes).

use crate::conv::EngineEvent;
use crate::outcome::EventId;

/// Default maximum ingest-log entries retained (ADR-0058 R2.4). The append-time
/// trim keeps `[latest_seq - DEFAULT_LOG_MAX_ENTRIES + 1, latest_seq]`, modulo
/// any still-eligible protected-cursor claim.
pub const DEFAULT_LOG_MAX_ENTRIES: u64 = 10_000;

/// `nmp_meta` key holding the seq-keyed ingest-log GC floor.
pub(crate) const KEY_INGEST_GC_FLOOR: &str = "ingest_gc_floor";

/// Why an event was semantically removed (ADR-0058 §3). LRU eviction and
/// `delete_by_filter(ByRelayOnly)` emit NO row — those are retention removals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteReason {
    /// NIP-09 author-signed kind:5 delete.
    Nip09,
    /// NIP-40 expiration reached (store-side reap).
    Nip40Expiry,
    /// Operator / admin purge via `delete_by_filter(ByAuthor|ByIds|ByKindRange)`.
    AdminPurge,
}

impl DeleteReason {
    /// The stable string stored in the `ingest_log.reason` column.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Nip09 => "Nip09",
            Self::Nip40Expiry => "Nip40Expiry",
            Self::AdminPurge => "AdminPurge",
        }
    }

    /// Decode the `reason` column back into a [`DeleteReason`], or `None` if the
    /// stored string is unrecognised.
    #[must_use]
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "Nip09" => Some(Self::Nip09),
            "Nip40Expiry" => Some(Self::Nip40Expiry),
            "AdminPurge" => Some(Self::AdminPurge),
            _ => None,
        }
    }
}

/// The operation recorded in a [`StoreLogEntry`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogOp {
    /// A new distinct event was accepted and stored.
    Inserted,
    /// A replaceable event superseded an older one.
    Replaced {
        /// The event id that was overwritten.
        replaced_id: EventId,
    },
    /// An event was semantically removed.
    Deleted {
        /// The event id that was removed.
        target_id: EventId,
        /// Why it was removed.
        reason: DeleteReason,
    },
}

/// One entry in the store's ingest log (ADR-0058 §3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreLogEntry {
    /// Monotonic ingest-order sequence (never reused, starts at 1).
    pub seq: u64,
    /// What happened.
    pub op: LogOp,
    /// For Inserted/Replaced the new event id; for Deleted the kind:5 carrier id.
    pub event_id: EventId,
    /// Raw event — `Some` for Inserted/Replaced, `None` for Deleted.
    pub raw_event: Option<EngineEvent>,
    /// The relay that delivered this event, or `None` for store-internal ops.
    pub source_relay: Option<String>,
    /// Wall-clock milliseconds when the event arrived.
    pub received_at_ms: u64,
}

/// A page of log entries returned by `scan_log_since_seq`.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Explicit gap — returned when `after_seq` is behind the GC floor. Never a
/// silent skip (ADR-0058 §6 `GapAllowed` contract).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullGap {
    /// The `after_seq` the caller requested.
    pub requested_after_seq: u64,
    /// Lowest seq still available; reset the cursor to this value.
    pub first_available_seq: u64,
}

/// Return type of `scan_log_since_seq`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanLogResult {
    /// Normal (possibly partial) page.
    Page(PullPage),
    /// Requested position was GC'd — explicit gap, never a silent skip.
    Gap(PullGap),
}

/// A `Protected`-cursor log-retention claim (ADR-0058 §6). VOLATILE — the kernel
/// replaces the whole set each pass. A claim pins the seq-keyed GC floor to
/// `after_seq` only while the cursor's lag stays within `max_lag_entries`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogRetentionClaim {
    /// The protected cursor's consumed position.
    pub after_seq: u64,
    /// Maximum lag before the claim is dropped.
    pub max_lag_entries: u64,
}

// The wasm SQLite paths (append/trim + the four read/claim methods) live in
// `ingest_log_store`; re-export the append helpers `insert`/`delete` call.
#[cfg(target_arch = "wasm32")]
pub(crate) use crate::ingest_log_store::{append_deleted, append_inserted, append_replaced};
