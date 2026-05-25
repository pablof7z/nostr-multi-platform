//! Opaque handle returned by `nmp_app_podcast_register` and consumed by
//! `nmp_app_podcast_snapshot` / `nmp_app_podcast_unregister`.

use nmp_ffi::NmpApp;

/// Opaque handle returned by [`super::nmp_app_podcast_register`]. Boxed on the
/// heap so the address is stable; the Swift consumer holds the raw pointer
/// until it calls [`super::nmp_app_podcast_unregister`].
///
/// At M0.A there is no podcast-specific projection to hold, so the handle
/// carries only the `app` back-pointer for unregister hygiene. Podcast-
/// specific state fields land here as later milestones ship.
pub struct PodcastHandle {
    // Retained for unregister hygiene; future milestones use it to call
    // `app_ref.unregister_event_observer(...)` for each registered projection.
    #[allow(dead_code)]
    pub(super) app: *mut NmpApp,
}

// SAFETY: the auto-derived `!Send`/`!Sync` comes solely from the `app: *mut
// NmpApp` field. The handle is sound to mark `Send + Sync` because:
//
//   1. Swift owns this handle and only ever touches it from one isolation
//      context (typically `@MainActor`). The FFI entry points are never raced.
//      This is a Swift-side caller convention, not a type-system guarantee.
//   2. The `app` raw pointer is only ever read — never mutated — from this
//      handle. Use-after-free is prevented by the `nmp_app_free` actor-join
//      fence: `Drop` sends `Shutdown` and joins the actor thread before
//      freeing the allocation, so any in-flight kernel callbacks have settled.
//
// CALLER CONTRACT: `nmp_app_free` must not be invoked while the handle is
// live. Call `nmp_app_podcast_unregister` before `nmp_app_free`.
unsafe impl Send for PodcastHandle {}
unsafe impl Sync for PodcastHandle {}
