//! `nmp-app-chirp` — Chirp per-app glue.
//!
//! Composes `nmp-core` (the kernel substrate + event observer slot, T146)
//! with `nmp-nip01` (the NIP-10 modular timeline view) and `nmp-threading`
//! (the agnostic grouping algorithm) to surface Twitter-style stacked-reply
//! modules over the kernel's home timeline. Lives outside `nmp-core` because
//! ADR-0009 forbids `nmp-core -> nmp-nip01` (cycle).
//!
//! ## Wiring
//!
//! The iOS shell calls [`ffi::nmp_app_chirp_register`] once at startup
//! (after `nmp_app_new` succeeds). That call:
//!
//! 1. Builds a [`state::ChirpModularTimeline`] with the viewer's pubkey and
//!    the default `ModulePolicy` (3-event modules, 72h gap threshold).
//! 2. Registers it as a kernel event observer via
//!    [`nmp_core::NmpApp::register_event_observer`]. From that moment on,
//!    every kind:1 the kernel ingests fans out to the projection.
//! 3. Returns an opaque handle the shell keeps for snapshots / unregister.
//!
//! On each render tick the shell calls [`ffi::nmp_app_chirp_snapshot`],
//! decodes the JSON, and renders `[TimelineBlock]` over the home timeline.
//!
//! ## Doctrine
//!
//! * **D0** — kernel emits, this crate composes. No business logic in
//!   Swift; the grouping algorithm is in `nmp-threading`.
//! * **D6** — every FFI symbol degrades silently on null pointers, lock
//!   poisoning, or serialization failure.

pub mod ffi;
pub mod marmot;
pub mod payload;
pub mod state;

pub use ffi::{
    nmp_app_chirp_register, nmp_app_chirp_snapshot, nmp_app_chirp_snapshot_free,
    nmp_app_chirp_unregister, ChirpHandle,
};
pub use payload::{ChirpEventCard, ChirpTimelineSnapshot};
pub use state::ChirpModularTimeline;

// ── Marmot (MLS encrypted groups) projection ─────────────────────────────
//
// A second FFI projection over the same kernel substrate. Mirrors the
// timeline symbols' naming / lifetime / free conventions. The iOS agent
// links these alongside the timeline symbols. Chirp owns ONLY the C-ABI
// shell ([`marmot::ffi`]); all business logic lives in
// `nmp_marmot::projection` (the reusable-from-any-host proof). The Rust
// type re-exports below resolve to that canonical home.
pub use marmot::ffi::{
    nmp_app_chirp_marmot_dispatch, nmp_app_chirp_marmot_group_messages,
    nmp_app_chirp_marmot_register, nmp_app_chirp_marmot_snapshot,
    nmp_app_chirp_marmot_string_free, nmp_app_chirp_marmot_unregister, MarmotHandle,
};
pub use nmp_marmot::projection::payload::{
    KeyPackageStatus, MarmotGroupRow, MarmotMessageRow, MarmotSnapshot, PendingWelcomeRow,
};
pub use nmp_marmot::projection::state::MarmotProjection;
