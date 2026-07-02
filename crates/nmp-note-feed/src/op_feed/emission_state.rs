//! ADR-0070 Rung 6 — feed emission state (thin re-export wrapper).
//!
//! The generic omit mechanism ([`TypedProjectionEmissionState`] +
//! [`FrameIdentity`]) was **generalised into `nmp-core`** in R6-S2 so that
//! `refs.event.envelopes` (registered in `nmp-native-runtime`) and `nip46_onboarding`
//! (registered in `nmp-core/actor`) can reuse the same byte-equality omit logic
//! without duplication. This module re-exports those types from
//! `nmp_core::projection_emission` and provides [`FeedEmissionState`] as a type
//! alias so the `explicit composition` composition root and all existing tests continue to
//! compile with zero changes.
//!
//! ## Design note (single implementation)
//!
//! There is exactly ONE omit implementation in the codebase:
//! `nmp_core::projection_emission::TypedProjectionEmissionState`. All three
//! typed projections that use the omit mechanism
//! (app-owned NNFS OP-feed projections, `refs.event.envelopes`,
//! `nip46_onboarding`) share it.
//!
//! ## Original mechanism docs (now in `nmp-core::projection_emission`)
//!
//! * Exact byte equality (memcmp), NOT a hash (collision = stale UI).
//! * Gated on `incremental_apply_enabled` (omit only when host retains;
//!   capability-OFF = always emit = byte-identical to today).
//! * Rebaseline on `(session_id, snapshot_epoch)` FrameIdentity in lockstep
//!   with the host `ProjectionCache.removeAll()` — the R6-S1 freeze fix.
//! * Monotonic `emit_rev` so the host reorder guard stays orthogonal.
//! * First frame after advertise / epoch / session change = full baseline.

// Re-export the generic types so `nmp-nip01` callers name them without
// reaching into `nmp-core` directly.
pub use nmp_core::projection_emission::{FrameIdentity, TypedProjectionEmissionState};

/// Per-producer emission state for NNFS OP-feed typed projections.
///
/// Type alias for [`TypedProjectionEmissionState`]; the feed is a whole-value
/// projection, so the generic state is used directly. All constructor and
/// method calls pass through unchanged — existing `op_feed_session.rs` code
/// and tests need zero modifications.
pub type FeedEmissionState = TypedProjectionEmissionState;

#[cfg(test)]
#[path = "emission_state_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "emission_state_host_tests.rs"]
mod host_tests;
