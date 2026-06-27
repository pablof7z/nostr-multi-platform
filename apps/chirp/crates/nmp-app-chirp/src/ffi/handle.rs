//! Opaque handle returned by `nmp_app_chirp_register` and consumed by
//! `nmp_app_chirp_unregister`.

use std::sync::Arc;

use nmp_ffi::NmpApp;
use nmp_nip01::OpFeedEngine;

/// Opaque handle returned by [`super::nmp_app_chirp_register`]. Boxed on the heap
/// so the address is stable; the Swift consumer holds the raw pointer until
/// it calls [`super::nmp_app_chirp_unregister`].
///
/// V-80 rung 7 — the handle now owns the OP-centric feed engine (a
/// [`nmp_nip01::OpFeedEngine`], `RootIndexedFeed<…>`) instead of the old
/// `ModularTimelineProjection`. Identity changes are handled by the app-
/// template registration through `NmpApp`'s Rust-side identity observer.
///
/// The engine and follow set are registered with the kernel by
/// [`nmp_native_runtime::register_op_feed_defaults`], which plugs their declared
/// observed projections into the kernel's standard `ObservedProjectionSink`
/// registry (not a single swappable slot). Those registrations live for the life of the `NmpApp`:
/// [`super::nmp_app_chirp_unregister`] no longer holds a single `observer_id`
/// to revoke — the `nmp_app_free` actor join is the fence that makes any
/// in-flight callback safe (see the `unsafe impl` rationale below).
pub struct ChirpHandle {
    /// The OP-feed engine. Snapshotted via [`ChirpHandle::snapshot`].
    pub(super) engine: Arc<OpFeedEngine>,
    /// The originating `NmpApp`. Retained to document the lifetime contract
    /// (`nmp_app_free` must outlive the handle) and so a future
    /// `unregister`-time teardown has the app to reach. Not dereferenced after
    /// the rung-7 cut-over — the kernel owns the engine/follow-set observer
    /// registrations and tears them down on `nmp_app_free`.
    #[allow(dead_code)]
    pub(super) app: *mut NmpApp,
}

// SAFETY: the auto-derived `!Send`/`!Sync` comes solely from the `app: *mut
// NmpApp` field (the `Arc<OpFeedEngine>` / `Arc<ActiveFollowSet>` are already
// `Send + Sync`). The handle is sound to mark `Send + Sync` because of three
// layered facts — stated honestly, since the previously-claimed "Swift
// serializes every FFI call on one thread" is NOT true (`KernelHandle` is a
// plain `final class` with no dispatch queue):
//
//   1. Swift owns this handle and only ever touches it from one isolation
//      context. In Chirp the FFI entry points below are reached exclusively
//      from `@MainActor` types (`KernelModel`, `MarmotStore`), so the handle
//      itself is never raced. (This is a Swift-side caller convention, not a
//      type-system guarantee — hence it is documented, not enforced here.)
//   2. The `Arc<OpFeedEngine>` *is* genuinely shared across threads: the
//      kernel actor thread invokes the engine's observer callbacks while the
//      Swift main actor calls `snapshot()`. Soundness of that sharing comes
//      from the engine's own interior `Mutex`, NOT from this `unsafe impl`.
//   3. The `app` raw pointer is only ever *read* — never mutated, and never
//      dereferenced from a kernel callback. The use-after-free question is
//      "can a callback touch `app` after `nmp_app_free`?" — and it cannot:
//      `nmp_app_free` drops `NmpApp`, whose `Drop` sends `Shutdown` and then
//      `join()`s the actor thread before the allocation is freed. The Rust
//      observer fan-out (`notify_observers`) invokes `on_kernel_event`
//      INLINE on that actor thread, so the join fences any in-flight
//      callback.
//
// CALLER CONTRACT: `nmp_app_free` must not be invoked while any kernel
// callback that reaches this handle's engine is still in flight. The
// in-process Rust-trait registration path used here gets that fence for free
// (the actor join). There is no C-ABI event observer path.
unsafe impl Send for ChirpHandle {}
unsafe impl Sync for ChirpHandle {}

impl ChirpHandle {
    /// Snapshot the OP-feed engine into the OP-centric
    /// [`crate::OpFeedSnapshot`] (`RootFeedSnapshot`). V-80 rung 7
    /// repointed the handle from the old `ModularTimelineProjection` to the
    /// `OpFeedEngine`; callers such as the REPL use this directly.
    pub fn snapshot(&self) -> crate::OpFeedSnapshot {
        // Use `snapshot_current_window()` to respect the engine's `window_limit`
        // (updated by `load_older`). `FeedRequest::default()` would hardcode
        // `DEFAULT_FEED_WINDOW_LIMIT` and ignore any load-older window growth.
        self.engine.snapshot_current_window()
    }
}
