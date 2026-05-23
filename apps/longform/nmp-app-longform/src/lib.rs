//! `nmp-app-longform` — a read-only NIP-23 long-form (kind:30023) article reader
//! built entirely on `nmp-core`'s substrate seams.
//!
//! # Why this crate exists
//!
//! This is the **second-app spike**: a falsification test of the framework
//! thesis that any developer can build a non-social NMP app by composing
//! substrate primitives (`NmpApp`, `KernelEventObserver`,
//! `register_snapshot_projection`, `push_interest`) without forking Chirp,
//! touching `nmp-app-chirp`, or editing `nmp-core`. See `apps/longform/README.md`
//! for the verdict.
//!
//! # Shape
//!
//! Process-global singleton (matches the no-handle FFI signatures in
//! `nmp_app_longform_init` / `nmp_app_longform_snapshot_json`):
//!
//! * [`projection::LongformProjection`] — a [`nmp_core::KernelEventObserver`]
//!   that collects accepted kind:30023 events into a deduped, sorted store.
//! * [`ffi`] — the C-ABI surface the host links against.
//!
//! # D0 — no Chirp deps
//!
//! `Cargo.toml` depends only on `nmp-core` + `serde` + `serde_json`. The crate
//! never names `nmp-app-chirp`, `nmp-nip23`, or any social/iOS surface.

pub mod ffi;
pub mod projection;

pub use projection::{Article, LongformProjection};
