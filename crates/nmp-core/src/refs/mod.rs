//! ADR-0063 Lane A — row-grain delta carrier for keyed reference projections.
//!
//! Issue #1671 unifies profile/event resolution into one kernel-owned
//! `RefResolver` primitive and lands FULL per-key reactivity (owner decision,
//! overriding codex's defer). This module is **Lane A**: the wire-transport
//! row-delta carrier + the producer-side row-rev tracker + a reference
//! host-cache model + the invariant property-test harness that is the merge
//! gate for the whole campaign.
//!
//! ## What this module owns
//!
//! - [`rowdelta`] — the owned `RefRow` / `RefRowDeltaBatch` types and their
//!   lossless FlatBuffers encode/decode over `schema/ref_rowdelta.fbs`. This is
//!   the OPAQUE `TypedPayload.payload` of a keyed `refs.profile` / `refs.event`
//!   projection (see the schema doc-comment for why it is NOT in
//!   `nmp_update.fbs`).
//! - [`tracker`] — [`tracker::RefRowDeltaTracker`], the producer that turns a
//!   per-key rev source ([`tracker::RefRowRevSource`], **Lane B's interface**)
//!   into a steady-state incremental batch (only changed/cleared rows) or a
//!   full baseline batch (every live row) under the ADR-0063 invariants.
//! - [`cache`] — [`cache::RefRowCache`], the reference model of the generated
//!   host-side per-key cache. The generated Swift / Kotlin keyed caches
//!   (`nmp-codegen`) implement the SAME algorithm; this Rust model is what the
//!   property harness checks `incremental-applied == full-snapshot` against.
//!
//! ## Lane B dependency (stubbed here)
//!
//! Lane B exposes the per-key rev map and resolved payloads. Lane A consumes it
//! through the [`tracker::RefRowRevSource`] trait ONLY — it never reimplements
//! the resolver. Until Lane B lands on the integration branch the trait is
//! satisfied in tests by [`tracker::MapRowRevSource`], an in-memory stub with
//! the exact shape Lane A needs (`ref_row_rev` + live-key enumeration +
//! payload). When Lane B lands, its resolver implements `RefRowRevSource` and
//! the stub is deleted.

mod cache;
mod rowdelta;
mod tracker;

#[cfg(test)]
mod tests;

pub use cache::{RefRowApplyOutcome, RefRowCache};
pub use rowdelta::{
    decode_ref_row_delta_batch, encode_ref_row_delta_batch, RefRow, RefRowDeltaBatch,
    RefRowDeltaDecodeError, RefRowState,
};
pub use tracker::{RefRowDeltaTracker, RefRowRevSource};
// `MapRowRevSource` is a TEST-ONLY in-memory Lane B stub (deleted when Lane B's
// real `RefResolver` lands); it is not part of the public crate surface.
#[cfg(test)]
pub(crate) use tracker::MapRowRevSource;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "generated/ref_rowdelta_generated.rs"]
mod ref_rowdelta_generated;

pub(crate) use ref_rowdelta_generated::nmp::refs as wire;
