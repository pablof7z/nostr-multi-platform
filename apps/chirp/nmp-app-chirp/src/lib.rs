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
//! The iOS shell links this one aggregate static library for Chirp. Keeping
//! `nmp-ffi`, the NIP-46 broker adapter, and the Chirp projection in one Rust
//! archive gives the process exactly one copy of the native C-ABI state.
//!
//! The shell calls [`nmp_signer_broker_init`] once after `nmp_app_new`, then
//! calls [`ffi::nmp_app_chirp_register`]. The projection registration:
//!
//! 1. Builds a reusable `nmp_nip01::ModularTimelineProjection` with the
//!    viewer's pubkey and the default `ModulePolicy`.
//! 2. Registers it as a kernel event observer via
//!    [`nmp_core::NmpApp::register_event_observer`]. From that moment on,
//!    every kind:1 the kernel ingests fans out to the projection.
//! 3. Returns an opaque handle the shell keeps for snapshots / unregister.
//!
//! On each render tick the shell calls [`ffi::nmp_app_chirp_snapshot_window`],
//! decodes the bounded JSON window, and renders `[TimelineBlock]` over the
//! home timeline.
//!
//! ## Doctrine
//!
//! * **D0** — kernel emits, this crate composes. No business logic in
//!   Swift; the grouping algorithm is in `nmp-threading`.
//! * **D6** — every FFI symbol degrades silently on null pointers, lock
//!   poisoning, or serialization failure.

pub mod ffi;
#[cfg(feature = "wallet")]
mod wallet_runtime;

pub use ffi::{
    nmp_app_chirp_default_window_limit, nmp_app_chirp_max_window_limit, nmp_app_chirp_register,
    nmp_app_chirp_snapshot, nmp_app_chirp_snapshot_free, nmp_app_chirp_snapshot_window,
    nmp_app_chirp_unregister, ChirpHandle,
};
pub use nmp_ffi::{
    nmp_app_cancel_bunker_handshake, nmp_app_nostrconnect_uri, nmp_broker_free_string,
    nmp_signer_broker_init,
};
pub use nmp_nip01::{
    ModularTimelineProjection as ChirpModularTimeline,
    ModularTimelineSnapshot as ChirpTimelineSnapshot, TimelineEventCard as ChirpEventCard,
    TimelineWindowCursor as ChirpTimelineWindowCursor,
    TimelineWindowMetrics as ChirpTimelineWindowMetrics,
    TimelineWindowPage as ChirpTimelineWindowPage,
    TimelineWindowRequest as ChirpTimelineWindowRequest, DEFAULT_TIMELINE_WINDOW_LIMIT,
    MAX_TIMELINE_WINDOW_LIMIT,
};

// ── Marmot (MLS encrypted groups) projection ─────────────────────────────
//
// A second FFI projection over the same kernel substrate. Mirrors the
// timeline symbols' naming / lifetime / free conventions. The iOS agent
// links these alongside the timeline symbols.
//
// The reusable C-ABI shell lives in the `nmp-marmot` crate
// (`crates/nmp-marmot/src/ffi.rs` + siblings) so the crate is a standalone
// buildable target for a future Marmot-only app. Chirp pulls it in via the
// `nmp-marmot/ffi` feature; the `#[no_mangle] nmp_marmot_*` symbols flow
// through `libnmp_app_chirp.a` automatically via rlib linkage (iOS still
// links exactly one staticlib). Chirp-specific identity/keyring wrappers stay
// in this app crate so `nmp-marmot` does not own Chirp symbol names or
// keyring account policy.
//
// Gated behind the `marmot` feature: MLS-over-Nostr was formally deferred to
// post-v1. Chirp opts in via its default feature set; a no-default-features
// build excludes the whole projection (dependency, modules, and FFI symbols).
#[cfg(feature = "marmot")]
pub use ffi::{
    nmp_app_chirp_identity_remove_account, nmp_app_chirp_identity_restore,
    nmp_app_chirp_identity_sign_in_nsec,
};
#[cfg(feature = "marmot")]
pub use nmp_marmot::fetch::nmp_marmot_fetch_key_packages;
#[cfg(feature = "marmot")]
pub use nmp_marmot::ffi::{
    nmp_marmot_group_messages, nmp_marmot_register, nmp_marmot_register_active,
    nmp_marmot_snapshot, nmp_marmot_string_free, nmp_marmot_unregister, MarmotHandle,
};
#[cfg(feature = "marmot")]
pub use nmp_marmot::projection::payload::{
    KeyPackageStatus, MarmotGroupRow, MarmotMessageRow, MarmotSnapshot, PendingWelcomeRow,
};
#[cfg(feature = "marmot")]
pub use nmp_marmot::projection::state::MarmotProjection;
