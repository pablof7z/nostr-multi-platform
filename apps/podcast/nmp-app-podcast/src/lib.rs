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
//! The host shell calls [`ffi::nmp_app_podcast_register`] once at startup.
//! On subscribe, the shell calls `nmp_app_podcast_subscribe` to record the
//! feed URL. The host platform (Android/iOS) then fetches the feed bytes and
//! passes them to `nmp_app_podcast_ingest_bytes` — which parses the
//! RSS/Atom body via `podcast-feeds` and populates the episode table.
//!
//! The snapshot carries real metadata + `episode_count` after ingest.
//! `nmp_app_podcast_episodes(handle, podcast_id)` returns the full episode
//! list for one podcast as a `FeedView` JSON string.
//!
//! ## HTTP-fetch gap (T-podcast-gap-3)
//!
//! Feed fetching is NOT performed by this crate. See `state.rs` and
//! `docs/perf/m11/T-podcast-gap-3.md`.
//!
//! ## Doctrine
//!
//! * **D0** — kernel stays podcast-agnostic; this crate composes domain.
//! * **D6** — every FFI symbol degrades silently on null pointers, lock
//!   poisoning, or serialization failure.
//! * **No business logic in shells** — JSON string → decode → render only.

pub mod ffi;
pub mod state;

pub use ffi::{
    nmp_app_podcast_episodes, nmp_app_podcast_ingest_bytes, nmp_app_podcast_register,
    nmp_app_podcast_snapshot, nmp_app_podcast_snapshot_free, nmp_app_podcast_subscribe,
    nmp_app_podcast_unregister, nmp_app_podcast_unsubscribe, PodcastHandle,
};
pub use state::{IngestResult, PodcastApp, SubscribeResult};
