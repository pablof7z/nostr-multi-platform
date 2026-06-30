//! Supporting types for `EventStore`.
//!
//! These types live here and are re-exported from `nmp_core::store`.
//! They track the design in `docs/design/lmdb/trait.md`.

mod coverage;
mod errors;
mod events;
mod gc;
mod ids;
mod outcomes;
mod query;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use coverage::{
    coverage_key, coverage_key_parts, CoverageGuard, CoverageMatchFn, CoverageRow, COVERAGE_KEY_SEP,
};
pub use errors::{StoreError, VerifyError};
pub use events::{RawEvent, StoredEvent, VerifiedEvent};
pub use gc::{
    DeleteFilter, DumpFormat, DumpStats, GcBudget, GcReport, DEFAULT_DURABLE_EVENT_CEILING,
    GC_MAX_DURATION_MS, GC_MAX_EVENTS_PER_STEP,
};
pub(crate) use ids::{hex_to_event_id, is_relay_provenance_private};
pub use ids::{EventId, PubKey, RelayUrl};
pub use outcomes::{
    InsertOutcome, ProvenanceEntry, RejectReason, TargetInteractionCounts, TombstoneOrigin,
    TombstoneRow,
};
pub use query::StoreQuery;
