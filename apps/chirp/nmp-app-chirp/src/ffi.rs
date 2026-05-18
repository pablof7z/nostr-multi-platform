//! Chirp per-app FFI surface.
//!
//! Four `extern "C"` symbols Swift links against:
//!
//! - [`nmp_app_chirp_register`] — instantiate `ChirpModularTimeline` with the
//!   active viewer pubkey and register it as a kernel event observer on the
//!   supplied `NmpApp`. Returns an opaque handle (boxed projection +
//!   observer id) for later snapshots / unregister.
//! - [`nmp_app_chirp_snapshot`] — serialize the current `ChirpTimelineSnapshot`
//!   into a freshly-allocated nul-terminated JSON C string. Swift owns the
//!   pointer until it calls `nmp_app_chirp_snapshot_free`.
//! - [`nmp_app_chirp_snapshot_free`] — companion deallocator for the snapshot
//!   string.
//! - [`nmp_app_chirp_unregister`] — drop the observer registration and free
//!   the handle. Idempotent.
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

use std::ffi::{c_char, CStr, CString};
use std::sync::Arc;

use nmp_core::{KernelEventObserverId, NmpApp};
use nmp_nip01::meta_timeline::Pubkey;
use nmp_nip01::ModularTimelineSpec;
use nmp_threading::ModulePolicy;

use crate::state::ChirpModularTimeline;

/// Opaque handle returned by [`nmp_app_chirp_register`]. Boxed on the heap
/// so the address is stable; the Swift consumer holds the raw pointer until
/// it calls [`nmp_app_chirp_unregister`].
pub struct ChirpHandle {
    projection: Arc<ChirpModularTimeline>,
    observer_id: KernelEventObserverId,
    app: *mut NmpApp,
}

// SAFETY: `ChirpHandle` is owned by Swift; only the `app` pointer is `!Send`/
// `!Sync` material. Swift drives every call from a single bridge dispatch
// queue (`KernelBridge.swift` already serializes its FFI calls on the actor
// thread for `NmpApp` itself), so cross-thread mutation does not happen.
// Marking these allows the handle to be boxed without extra unsafe gymnastics
// in the Rust callsites below.
unsafe impl Send for ChirpHandle {}
unsafe impl Sync for ChirpHandle {}

/// Register a Chirp modular timeline projection against `app`. Returns a
/// non-null `*mut ChirpHandle` on success; `null` on any failure (null
/// pointer arguments, invalid UTF-8 viewer pubkey, slot lock poisoning).
///
/// `viewer_pubkey` is a hex-encoded pubkey (typically 64 chars; not
/// validated here — the grouper carries it through unchanged for future
/// personalization keys). NULL is permitted and treated as "no viewer".
///
/// `app` MUST outlive the returned handle. Call [`nmp_app_chirp_unregister`]
/// before `nmp_app_free`.
#[no_mangle]
pub extern "C" fn nmp_app_chirp_register(
    app: *mut NmpApp,
    viewer_pubkey: *const c_char,
) -> *mut ChirpHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `app` is a valid pointer allocated by
    // `nmp_app_new` for the duration of this call. We do not hold the
    // borrow past this function.
    let app_ref = unsafe { &*app };

    let viewer: Pubkey = c_string_opt(viewer_pubkey).unwrap_or_default();
    let spec = ModularTimelineSpec {
        viewer,
        kinds: Vec::new(),
        authors: None,
        policy: ModulePolicy::default(),
    };

    let projection = Arc::new(ChirpModularTimeline::new(spec));
    let observer_id = app_ref.register_event_observer(Arc::clone(&projection)
        as Arc<dyn nmp_core::KernelEventObserver>);
    if observer_id.0 == 0 {
        // Registration failed (poisoned mutex). Don't leak the projection;
        // caller gets a null handle and treats it as a soft-fail.
        return std::ptr::null_mut();
    }

    Box::into_raw(Box::new(ChirpHandle {
        projection,
        observer_id,
        app,
    }))
}

/// Serialize the current `ChirpTimelineSnapshot` into a JSON C string.
/// Returns null on any failure (null handle, JSON encode error, CString
/// nul-byte conflict). The returned pointer is owned by the caller; pass it
/// to [`nmp_app_chirp_snapshot_free`] when done.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_snapshot(handle: *mut ChirpHandle) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `handle` is a valid pointer returned by
    // `nmp_app_chirp_register` and not yet freed.
    let handle = unsafe { &*handle };
    let snapshot = handle.projection.snapshot();
    let Ok(payload) = serde_json::to_string(&snapshot) else {
        return std::ptr::null_mut();
    };
    let Ok(cstr) = CString::new(payload) else {
        return std::ptr::null_mut();
    };
    cstr.into_raw()
}

/// Free a snapshot string previously returned by
/// [`nmp_app_chirp_snapshot`]. Null pointer is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_snapshot_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ptr` came from `CString::into_raw` in
    // `nmp_app_chirp_snapshot` and has not been freed.
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

/// Drop the projection's observer registration and free the handle.
/// Idempotent: null pointer is a silent no-op. The handle MUST NOT be used
/// after this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_unregister(handle: *mut ChirpHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from `nmp_app_chirp_register`
    // and has not already been freed.
    let boxed = unsafe { Box::from_raw(handle) };
    if !boxed.app.is_null() {
        // SAFETY: same `app` validity rule as `nmp_app_chirp_register` — the
        // caller is responsible for the `nmp_app_free` ordering invariant.
        let app_ref = unsafe { &*boxed.app };
        app_ref.unregister_event_observer(boxed.observer_id);
    }
    // boxed dropped here — projection's last Arc released only if no other
    // strong refs exist (none should once the observer is unregistered).
}

fn c_string_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` (when non-null) is a valid
    // nul-terminated C string for the duration of this call.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(|s| s.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::nmp_app_free;
    use nmp_core::nmp_app_new;

    #[test]
    fn register_snapshot_unregister_round_trip() {
        let app = nmp_app_new();
        let handle = nmp_app_chirp_register(app, std::ptr::null());
        assert!(!handle.is_null(), "register returned null");

        // Empty snapshot — no events have arrived.
        let snap = nmp_app_chirp_snapshot(handle);
        assert!(!snap.is_null());
        // SAFETY: snap is a valid C string from our own CString.
        let json = unsafe { CStr::from_ptr(snap) }.to_str().unwrap().to_owned();
        nmp_app_chirp_snapshot_free(snap);
        // Empty snapshot decodes to empty arrays.
        assert!(json.contains("\"blocks\":[]"));
        assert!(json.contains("\"cards\":[]"));

        nmp_app_chirp_unregister(handle);
        nmp_app_free(app);
    }

    #[test]
    fn null_handle_paths_are_silent_noops() {
        nmp_app_chirp_unregister(std::ptr::null_mut());
        let snap = nmp_app_chirp_snapshot(std::ptr::null_mut());
        assert!(snap.is_null());
        nmp_app_chirp_snapshot_free(std::ptr::null_mut());
    }

    #[test]
    fn register_with_null_app_returns_null() {
        let handle = nmp_app_chirp_register(std::ptr::null_mut(), std::ptr::null());
        assert!(handle.is_null());
    }
}
