//! Marmot (MLS-over-Nostr) per-app projection for Chirp.
//!
//! A second FFI projection alongside the NIP-10 modular timeline, built to
//! the SAME shape as `crate::{ffi, state, payload}`:
//!
//! * [`payload`] — flat, decoder-free DTOs (the iOS shell mirrors the
//!   serde shape verbatim).
//! * [`state`] — `MarmotProjection`: owns the `nmp-marmot`
//!   `MarmotService` + the FFI-local bookkeeping it does not surface
//!   (pending-welcome cache, key-package publish timestamp). Implements
//!   `KernelEventObserver` (metadata-only; see the lossy-observer seam).
//! * [`ops`] — dispatch + read-projection handlers; the ONLY place
//!   `mdk-core` input types are named (FFI translation-layer exception,
//!   documented in `Cargo.toml`).
//! * [`ffi`] — the six `#[no_mangle] extern "C"` symbols.
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on `nmp-marmot`; this crate is the
//!   composition point (ADR-0009).
//! * **D6** — every FFI symbol degrades silently (null / `{"ok":false}`),
//!   never panics across the boundary.

pub mod ffi;
pub mod ops;
pub mod payload;
pub mod state;
