//! Unregister entry point the host calls against a
//! [`ChirpHandle`] returned by [`super::register::nmp_app_chirp_register`].

use super::handle::ChirpHandle;

/// Free the handle.
/// Idempotent: null pointer is a silent no-op. The handle MUST NOT be used
/// after this call.
///
/// V-80 rung 7 — the OP-feed engine + `ActiveFollowSet` observers are
/// registered by `nmp_native_runtime::register_op_feed_defaults` through the
/// kernel's standard observer registry, NOT through a single swappable slot
/// this handle owns. There is no per-handle `observer_id` to revoke here; the
/// observers live for the life of the `NmpApp` and are torn down by
/// `nmp_app_free` (the actor `join()` fences any in-flight callback — see the
/// `ChirpHandle` `unsafe impl` rationale). Dropping the boxed handle releases
/// this crate's `Arc` clones of the engine and follow set; the kernel keeps
/// its own clones until `nmp_app_free`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_unregister(handle: *mut ChirpHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from `nmp_app_chirp_register`
    // and has not already been freed. Reclaim the box and drop it — releasing
    // this crate's `Arc` clones of the engine + follow set.
    let boxed = unsafe { Box::from_raw(handle) };
    // Purge any parked tag-feed handles owned by this app so a later app reusing
    // the same pointer address can never collide with a stale session id (the
    // app's session registry is dropped with it on `nmp_app_free`).
    super::tag_feed::forget_app_tags(boxed.app);
}
