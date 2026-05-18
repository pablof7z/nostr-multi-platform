//! Supporting types for `EventStore`.
//!
//! These types live here and are re-exported from `nmp_core::store`.
//! They track the design in `docs/design/lmdb/trait.md`.

mod ids;
mod events;
mod outcomes;
mod watermark;
mod gc;
mod errors;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use ids::{EventId, PubKey, RelayUrl};
pub use events::{RawEvent, VerifiedEvent, StoredEvent};
pub use outcomes::{InsertOutcome, RejectReason, TombstoneRow, TombstoneOrigin, ProvenanceEntry};
pub use watermark::{WatermarkKey, WatermarkRow, SyncMethod, Coverage};
pub use gc::{ClaimerId, GcBudget, GcReport, DeleteFilter, DumpFormat, DumpStats};
pub use errors::{StoreError, VerifyError};
