//! `nmp-app-podcast` — Podcast per-app glue (M0.A skeleton).
//!
//! Composes `nmp-core` (the kernel substrate) with the canonical NMP
//! composition root (`nmp-app-template`) to provide a stub FFI surface the
//! iOS Podcast shell can link against. Podcast-specific projections and action
//! modules land in later milestones (M1+); this crate is pure structural
//! scaffolding.
//!
//! ## Wiring
//!
//! The iOS shell links `libnmp_app_podcast.a`. It calls
//! [`nmp_signer_broker_init`] once after `nmp_app_new`, then calls
//! [`ffi::nmp_app_podcast_register`]. That registration:
//!
//! 1. Runs `nmp_app_template::register_defaults` — NIP-02 / NIP-17 / NIP-57 /
//!    NIP-65 action modules, kind:10050 ingest, routing substrate, DM-inbox
//!    and zap-receipts runtime controllers.
//! 2. Returns an opaque handle the shell keeps for snapshots / unregister.
//!
//! On each render tick the shell calls [`ffi::nmp_app_podcast_snapshot`],
//! decodes the JSON, and renders against the stub payload (schema_version=1).
//!
//! ## Doctrine
//!
//! * **D0** — kernel emits, this crate composes. No business logic in Swift.
//! * **D6** — every FFI symbol degrades silently on null pointers or other
//!   failure conditions.

pub mod ffi;

pub use ffi::{
    nmp_app_podcast_register, nmp_app_podcast_snapshot, nmp_app_podcast_snapshot_free,
    nmp_app_podcast_unregister, PodcastHandle,
};
pub use nmp_signer_broker::{
    nmp_app_cancel_bunker_handshake, nmp_app_nostrconnect_uri, nmp_broker_free_string,
    nmp_signer_broker_init,
};
