//! Marmot FFI projection layer — the typed translation layer a C-ABI /
//! actor consumer needs (opaque hex `group_id`, string errors, flat serde
//! DTOs). Migrated out of the Chirp app so any NMP app can reuse it; Chirp
//! is now a thin `#[no_mangle] extern "C"` shell over these modules.
//!
//! * [`payload`] — flat, decoder-free DTOs (a host shell mirrors the serde
//!   shape verbatim).
//! * [`state`] — [`state::MarmotProjection`]: owns the service + FFI-local
//!   bookkeeping (pending-welcome cache, key-package publish timestamp);
//!   implements `KernelEventObserver` (metadata-only).
//! * [`ops`] — dispatch + read-projection handlers; the ONLY place
//!   `mdk-core` input types are named for this layer.
//! * [`publish`] — the internal relay-publish bridge that CLOSES the
//!   outbound seam (calls the workspace-internal
//!   `nmp_core::NmpApp::publish_signed_explicit` kernel API against the
//!   retained `&NmpApp`).
//! * [`tap`] — the inbound raw-event observer that CLOSES the inbound
//!   ingest seam (drives accepted kind:1059/445 events through the shared
//!   `ops::ingest_signed_event_core`).

pub mod action;
pub mod display;
pub mod handler;
pub mod ops;
pub mod payload;
pub mod publish;
pub mod state;
pub mod tap;
