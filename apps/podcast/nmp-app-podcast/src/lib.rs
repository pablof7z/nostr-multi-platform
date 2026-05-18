//! `nmp-app-podcast` — Podcast per-app glue.
//!
//! Composes `nmp-core` (the kernel substrate + event observer slot) with
//! `podcast-core` (podcast domain records, view payloads, action shapes) to
//! surface a podcast `LibraryView` over an FFI static library. Lives outside
//! `nmp-core` because doctrine D0 forbids `nmp-core` from gaining podcast
//! nouns.
//!
//! ## Wiring
//!
//! The iOS shell calls [`ffi::nmp_app_podcast_register`] once at startup
//! (after `nmp_app_new` succeeds). That call builds a [`state::PodcastApp`]
//! and returns an opaque handle the shell holds for snapshots /
//! subscribe / unsubscribe / unregister.
//!
//! On each render tick (or after dispatch) the shell calls
//! [`ffi::nmp_app_podcast_snapshot`], decodes the JSON
//! [`podcast_core::views::LibraryView`], and renders the list.
//!
//! ## Doctrine
//!
//! * **D0** — kernel stays podcast-agnostic; this crate composes domain.
//! * **D6** — every FFI symbol degrades silently on null pointers, lock
//!   poisoning, or serialization failure.
//! * **No business logic in Swift** — Swift takes the JSON string, decodes
//!   to `LibraryView`, and renders. All state lives here.

pub mod ffi;
pub mod state;

pub use ffi::{
    nmp_app_podcast_register, nmp_app_podcast_snapshot, nmp_app_podcast_snapshot_free,
    nmp_app_podcast_subscribe, nmp_app_podcast_unregister, nmp_app_podcast_unsubscribe,
    PodcastHandle,
};
pub use state::PodcastApp;
