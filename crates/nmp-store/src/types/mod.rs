//! Supporting types for `EventStore`.
//!
//! These types live here and are re-exported from `nmp_core::store`.
//! They track the design in `docs/design/lmdb/trait.md`.

mod errors;
mod events;
mod gc;
mod ids;
mod outcomes;
mod query;
mod watermark;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use errors::{StoreError, VerifyError};
pub use events::{RawEvent, StoredEvent, VerifiedEvent};
pub use gc::{ClaimerId, DeleteFilter, DumpFormat, DumpStats, GcBudget, GcReport};
pub use ids::{EventId, PubKey, RelayUrl};
pub use outcomes::{InsertOutcome, ProvenanceEntry, RejectReason, TombstoneOrigin, TombstoneRow};
pub use query::StoreQuery;
pub use watermark::{
    Coverage, SyncMethod, WatermarkKey, WatermarkRow, COVERAGE_STALENESS_WINDOW_SECS,
};
