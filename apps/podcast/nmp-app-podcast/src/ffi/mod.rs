//! Podcast per-app FFI surface.
//!
//! `extern "C"` symbols Swift links against:
//!
//! - [`nmp_app_podcast_register`] — instantiate a `PodcastHandle`, run the
//!   canonical NMP composition (`register_defaults`), and return an opaque
//!   handle for later snapshots / unregister.
//! - [`nmp_app_podcast_snapshot`] — serialize the current stub snapshot into a
//!   freshly-allocated nul-terminated JSON C string. Swift owns the pointer
//!   until it calls `nmp_app_podcast_snapshot_free`.
//! - [`nmp_app_podcast_snapshot_free`] — companion deallocator for the
//!   snapshot string.
//! - [`nmp_app_podcast_unregister`] — drop the handle. Idempotent.
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on app-specific crates; this crate is
//!   the composition point.
//! * **D6** — every entry point is fire-and-forget. Null pointers and
//!   serialization failures degrade silently rather than raising across the FFI.
//! * **No business logic in Swift** — Swift decodes the JSON stub and renders.
//!   All logic moves here as milestones ship.
//!
//! ## Module layout
//!
//! Split across sub-modules to keep each file under the 500-LOC ceiling.
//! Every `pub extern "C"` symbol Swift links against is re-exported below.

mod actions;
mod handle;
mod register;
mod snapshot;

pub use handle::PodcastHandle;
pub use register::nmp_app_podcast_register;
pub use snapshot::{
    nmp_app_podcast_snapshot, nmp_app_podcast_snapshot_free, nmp_app_podcast_unregister,
};
