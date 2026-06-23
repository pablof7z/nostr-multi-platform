//! Lifecycle C-ABI entry points for `NmpApp` — extracted from `lib.rs` to
//! keep each file under the 500-LOC ceiling (AGENTS.md file-size rule).
//!
//! Covers: `nmp_app_free`, `nmp_app_set_update_callback`, `nmp_app_start`,
//! `nmp_app_configure`, `nmp_app_stop`, `nmp_app_reset`.

use std::ffi::c_void;
use std::sync::atomic::Ordering;

use nmp_core::ActorCommand;
use nmp_core::__ffi_internal::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};

use crate::app_ref;
use crate::app_struct::{NmpApp, UpdateCallback, UpdateCallbackRegistration};

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
    if !app.is_null() {
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
    let callback_registered = callback.is_some();
    let new_registration = callback.map(|callback| UpdateCallbackRegistration {
        context: context as usize,
        callback,
    });
    // Install the new registration (or clear) and then wait until any
    // in-flight invocation of the OLD registration has finished.
    let Ok(guard) = app.update_callback.inner.lock() else {
        return;
    };
    let mut guard = guard;
    guard.registration = new_registration;
    let waited = app
        .update_callback
        .drained
        .wait_while(guard, |inner| inner.in_flight > 0);
    // When `wait_while` returns, `in_flight == 0` under the lock. Dropping
    // `waited` releases the lock.
    drop(waited);
    if callback_registered && !app.started.load(Ordering::SeqCst) {
        app.emit_passive_prestart_snapshot();
    }
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

    // Read the pre-start initial relay configuration.
    let initial_relays = app
        .initial_relays_for_start
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();

    // ADR-0053 / Workstream-E4 — LOUD forgotten-declaration check.
    if app.consumed_projections_are_undeclared() {
        tracing::warn!(
            "nmp_app_start: host expressed no projection-consumption intent — the \
             kernel will serialize all {} Tier-2 built-in projections on every tick \
             (including relay_diagnostics). This is a FORGOTTEN declaration, not an \
             opt-in: call `nmp_app_declare_consumed_projections` (narrow) or \
             `nmp_app_consume_all_builtin_projections` (explicit full set) before \
             start (ADR-0053 / Workstream-E4).",
            nmp_core::KERNEL_BUILTIN_PROJECTION_KEYS.len(),
        );
        #[cfg(not(any(test, feature = "test-support")))]
        debug_assert!(
            false,
            "nmp_app_start: projection-consumption intent is undeclared — call \
             nmp_app_declare_consumed_projections (narrow) or \
             nmp_app_consume_all_builtin_projections (explicit everything) before \
             start. No silent emit-everything default (ADR-0053 / Workstream-E4)."
        );
    }

    // ADR-0049 Part 2 — mark the app started BEFORE sending Start.
    let was_started = app.started.swap(true, Ordering::SeqCst);
    if !was_started {
        app.spawn_actor_if_needed();
    }

    app.send_cmd(ActorCommand::Start {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
        initial_relays,
    });
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

    app.send_cmd(ActorCommand::Configure {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_stop(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::Stop);
}

#[no_mangle]
pub extern "C" fn nmp_app_reset(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::Reset);
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
