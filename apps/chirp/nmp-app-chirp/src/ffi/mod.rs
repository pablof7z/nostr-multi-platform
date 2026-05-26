//! Chirp per-app FFI surface.
//!
//! `extern "C"` symbols Swift links against:
//!
//! - [`nmp_app_chirp_register`] — instantiate `ChirpModularTimeline` with the
//!   active viewer pubkey and register it as a kernel event observer on the
//!   supplied `NmpApp`. Returns an opaque handle (boxed projection +
//!   observer id) for later snapshots / unregister.
//! - [`nmp_app_chirp_register_group_chat`] — wire a NIP-29
//!   `GroupChatProjection` for one group into the kernel: an event observer
//!   (ingest) plus a `"nmp.nip29.group_chat"` snapshot projection (output). Pure
//!   consumption — no handle, no actions, no unregister.
//! - [`nmp_app_chirp_register_dm_inbox`] — host entry point for the NIP-17 DM
//!   runtime. `nmp_app_chirp_register` wires it eagerly: a kind:1059
//!   raw-event observer, a `"nmp.nip17.dm_inbox"` snapshot projection, and a
//!   Rust-owned controller for the active gift-wrap interest + kind:10050
//!   relay-list publish.
//! - [`nmp_app_chirp_snapshot`] — serialize the current `ChirpTimelineSnapshot`
//!   into a freshly-allocated nul-terminated JSON C string. Swift owns the
//!   pointer until it calls `nmp_app_chirp_snapshot_free`.
//! - [`nmp_app_chirp_snapshot_window`] — serialize a bounded cursor window of
//!   the timeline for render shells that should not pull the full feed.
//! - [`nmp_app_chirp_snapshot_free`] — companion deallocator for the snapshot
//!   string.
//! - [`nmp_app_chirp_unregister`] — drop the observer registration and free
//!   the handle. Idempotent.
//! - `nmp_app_chirp_identity_restore`,
//!   `nmp_app_chirp_identity_sign_in_nsec`, and
//!   `nmp_app_chirp_identity_remove_account` — Chirp-owned identity/keyring
//!   wrappers that register the reusable Marmot projection without leaking
//!   Chirp policy into `nmp-marmot`.
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on `nmp-nip01`; this crate is the
//!   composition point. ADR-0009 (kernel boundary).
//! * **D6** — every entry point is fire-and-forget. Null pointers, missing
//!   strings, serialization failures, and poisoned mutexes all degrade
//!   silently rather than raising across the FFI.
//! * **No business logic in Swift** — Swift takes the JSON string, decodes
//!   to `[TimelineBlock] + [ChirpEventCard]`, and renders. All grouping
//!   happens here / in `nmp-threading`.
//!
//! ## Module layout
//!
//! This module is split across several sub-modules to keep each file under
//! the V-09 500-LOC hand-authored ceiling. The split is purely organizational —
//! every `pub extern "C"` symbol Swift links against is re-exported below so
//! the C-ABI surface is unchanged.

mod actions;
mod handle;
mod helpers;
#[cfg(feature = "marmot")]
mod identity;
mod register;
mod snapshot;

#[cfg(test)]
mod tests;

pub use handle::ChirpHandle;
#[cfg(feature = "marmot")]
pub use identity::{
    nmp_app_chirp_identity_remove_account, nmp_app_chirp_identity_restore,
    nmp_app_chirp_identity_sign_in_nsec,
};
pub use register::{
    nmp_app_chirp_register, nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list,
    nmp_app_chirp_register_group_chat, nmp_app_chirp_register_group_discovery,
};
pub use snapshot::{
    nmp_app_chirp_snapshot, nmp_app_chirp_snapshot_free, nmp_app_chirp_snapshot_window,
    nmp_app_chirp_unregister,
};
