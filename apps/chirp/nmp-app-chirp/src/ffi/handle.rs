//! Opaque handle returned by `nmp_app_chirp_register` and consumed by
//! `nmp_app_chirp_snapshot` / `nmp_app_chirp_unregister`.

use std::sync::Arc;

use nmp_core::KernelEventObserverId;
use nmp_ffi::NmpApp;
use nmp_nip01::ModularTimelineProjection;

/// Opaque handle returned by [`super::nmp_app_chirp_register`]. Boxed on the heap
/// so the address is stable; the Swift consumer holds the raw pointer until
/// it calls [`super::nmp_app_chirp_unregister`].
pub struct ChirpHandle {
    pub(super) projection: Arc<ModularTimelineProjection>,
    pub(super) observer_id: KernelEventObserverId,
    pub(super) app: *mut NmpApp,
}

// SAFETY: the auto-derived `!Send`/`!Sync` comes solely from the `app: *mut
// NmpApp` field (the `Arc<ChirpModularTimeline>` is already `Send + Sync`).
// The handle is sound to mark `Send + Sync` because of three layered facts —
// stated honestly, since the previously-claimed "Swift serializes every FFI
// call on one thread" is NOT true (`KernelHandle` is a plain `final class`
// with no dispatch queue):
//
//   1. Swift owns this handle and only ever touches it from one isolation
//      context. In Chirp the FFI entry points below are reached exclusively
//      from `@MainActor` types (`KernelModel`, `MarmotStore`), so the handle
//      itself is never raced. (This is a Swift-side caller convention, not a
//      type-system guarantee — hence it is documented, not enforced here.)
//   2. The `Arc<ModularTimelineProjection>` *is* genuinely shared across threads:
//      the kernel actor thread invokes `ModularTimelineProjection`'s observer
//      callbacks while the Swift main actor calls `snapshot()`. Soundness of
//      that sharing comes from the projection's own interior `Mutex`, NOT
//      from this `unsafe impl`.
//   3. The `app` raw pointer is only ever *read* — never mutated, and never
//      dereferenced from a kernel callback. The use-after-free question is
//      "can a callback touch `app` after `nmp_app_free`?" — and it cannot:
//      `nmp_app_free` drops `NmpApp`, whose `Drop` sends `Shutdown` and then
//      `join()`s the actor thread before the allocation is freed. The Rust
//      observer fan-out (`notify_observers`) invokes `on_kernel_event`
//      INLINE on that actor thread, so the join fences any in-flight
//      callback. Calling `nmp_app_chirp_unregister` before `nmp_app_free`
//      (the documented contract) is additional hygiene; the actor join is
//      the actual fence.
//
// CALLER CONTRACT: `nmp_app_free` must not be invoked while any kernel
// callback that reaches this handle's projection is still in flight. The
// in-process Rust-trait registration path used here gets that fence for free
// (the actor join). A hypothetical C-ABI observer would NOT — its drain
// thread is separate and is not joined by `nmp_app_free`.
unsafe impl Send for ChirpHandle {}
unsafe impl Sync for ChirpHandle {}

impl ChirpHandle {
    /// Return the current [`nmp_nip01::ModularTimelineSnapshot`] directly,
    /// without going through the C ABI. Safe to call from any thread that
    /// holds a valid pointer to this handle (same contract as the other
    /// methods on this type).
    pub fn snapshot(&self) -> nmp_nip01::ModularTimelineSnapshot {
        self.projection.snapshot()
    }
}
