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
//! * [`command`] — the typed [`command::MarmotProtocolCommand`] (#1940) that
//!   carries an already-parsed `MarmotAction` + the shared projection to
//!   `ops::dispatch`, plus the `ContextHostPort` bridge.
//! * [`host_port`] — the single outbound-effect seam (`MarmotHostPort`) ops
//!   use for publish / write-relay / interest / terminal-verdict; replaces
//!   the deleted `*mut NmpApp` raw pointer and the `projection::publish`
//!   bridge.
//! * [`tap`] — the inbound raw-event observer that CLOSES the inbound
//!   ingest seam (drives accepted kind:1059/445 events through the shared
//!   `ops::ingest_signed_event_core`).

pub mod action;
pub mod command;
pub mod deferred;
pub mod host_port;
pub mod ops;
pub mod payload;
pub mod pending;
pub mod resubscribe;
pub mod state;
pub mod tap;
