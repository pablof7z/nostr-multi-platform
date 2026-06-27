//! Lifecycle C-ABI entry points for `NmpApp` — extracted from `lib.rs` to
//! keep each file under the 500-LOC ceiling (AGENTS.md file-size rule).
//!
//! Covers: `nmp_app_free`, `nmp_app_set_update_callback`, `nmp_app_start`,
//! `nmp_app_configure`, `nmp_app_stop`, `nmp_app_reset`.

use std::ffi::c_void;
use std::sync::Arc;

use nmp_core::__ffi_internal::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};

use crate::{app_ref, NmpApp};

/// C update callback shape. The byte slice passed to the callback is valid
/// only for the callback duration.
pub type UpdateCallback = extern "C" fn(*mut c_void, *const u8, usize);

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
    if !app.is_null() {
        // `NmpApp::drop` clears the capability callback through the quiescence
        // gate before joining the actor, so actor/worker-owned capability
        // dispatches cannot keep executing against host context after free.
        // SAFETY: caller guarantees app is a valid pointer allocated by nmp_app_new().
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}

/// Register (or clear) the update callback on `app`.
///
/// # Quiescence contract
///
/// After this function returns, the previous `(callback, context)` pair is
/// guaranteed to be **neither registered nor mid-invocation**. Hosts may
/// safely free or release `context` once this call returns — no further
/// invocations of the old callback will occur, and any in-flight invocation
/// has completed before this function returns.
///
/// Pass `callback = None` (or `None` for the function pointer) to clear the
/// registration entirely. Passing a new `(context, callback)` pair replaces
/// the old one atomically from the perspective of the quiescence guarantee:
/// when this returns, the old registration is drained and the new one is
/// installed.
///
/// # Re-entrancy
///
/// A host callback **must not** call `nmp_app_set_update_callback` from
/// within the callback itself. The setter waits for in-flight invocations to
/// drain (via a `Condvar`), which cannot happen while the callback is
/// running on the listener thread — this would deadlock.
#[no_mangle]
pub extern "C" fn nmp_app_set_update_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<UpdateCallback>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let listener = callback.map(|callback| {
        let context = context as usize;
        Arc::new(move |bytes: &[u8]| {
            callback(context as *mut c_void, bytes.as_ptr(), bytes.len());
        }) as nmp_native_runtime::UpdateListener
    });
    app.set_update_listener(listener);
}

#[no_mangle]
pub extern "C" fn nmp_app_start(
    app: *mut NmpApp,
    visible_limit: std::ffi::c_uint,
    emit_hz: std::ffi::c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    app.start_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
}

#[no_mangle]
pub extern "C" fn nmp_app_configure(
    app: *mut NmpApp,
    visible_limit: std::ffi::c_uint,
    emit_hz: std::ffi::c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    app.configure_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
}

#[no_mangle]
pub extern "C" fn nmp_app_stop(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.stop_runtime();
}

#[no_mangle]
pub extern "C" fn nmp_app_reset(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.reset_runtime();
}

pub(crate) fn clamp_visible(visible_limit: std::ffi::c_uint) -> usize {
    if visible_limit == 0 {
        DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

pub(crate) fn clamp_emit_hz(emit_hz: std::ffi::c_uint) -> u32 {
    if emit_hz == 0 {
        DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}
