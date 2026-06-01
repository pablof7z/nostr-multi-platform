//! Chirp per-app FFI surface.
//!
//! `extern "C"` symbols Swift links against:
//!
//! - [`nmp_app_chirp_register`] — wire the OP-centric home feed (V-80 rung 7)
//!   via `nmp_app_template::register_op_feed_defaults`: the `nmp-nip01` OP-feed
//!   engine registered as both a kernel event observer (ingest) and a
//!   `"nmp.feed.home"` feed controller (output), plus the `ActiveFollowSet`
//!   producer. Returns an opaque handle (boxed engine + follow set) for later
//!   snapshots / unregister.
//! - [`nmp_app_chirp_register_group_chat`] — wire a NIP-29
//!   `GroupChatProjection` for one group into the kernel: an event observer
//!   (ingest) plus a `"nmp.nip29.group_chat"` snapshot projection (output). Pure
//!   consumption — no handle, no actions, no unregister.
//! - [`nmp_app_chirp_register_dm_inbox`] — host entry point for the NIP-17 DM
//!   runtime. `nmp_app_chirp_register` wires it eagerly: a kind:1059
//!   raw-event observer, a `"nmp.nip17.dm_inbox"` snapshot projection, and a
//!   Rust-owned controller for the active gift-wrap interest + kind:10050
//!   relay-list publish.
//! - [`nmp_app_chirp_unregister`] — free the handle. Idempotent. (The engine /
//!   follow-set observer registrations are torn down by `nmp_app_free`.)
//! - `nmp_app_chirp_identity_restore`,
//!   `nmp_app_chirp_identity_sign_in_nsec`, and
//!   `nmp_app_chirp_identity_remove_account` — Chirp-owned identity wrappers
//!   that register the reusable Marmot projection without leaking Chirp symbol
//!   policy into `nmp-marmot`.
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on `nmp-nip01`; this crate is the
//!   composition point. ADR-0009 (kernel boundary).
//! * **D6** — every entry point is fire-and-forget. Null pointers, missing
//!   strings, serialization failures, and poisoned mutexes all degrade
//!   silently rather than raising across the FFI.
//! * **No business logic in Swift** — Swift takes the JSON string, decodes
//!   the `RootFeedSnapshot` (`[{ card, attribution }]`), and renders. All
//!   root-indexing / attribution happens here / in `nmp-feed` + `nmp-nip01`.
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
pub use snapshot::nmp_app_chirp_unregister;
