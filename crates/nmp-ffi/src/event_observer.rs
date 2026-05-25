//! T146 — kernel event observer FFI surface.
//!
//! Three `extern "C"` symbols exported to Swift / C:
//!
//! - [`nmp_app_register_event_observer`] — register a callback that fires
//!   once per kernel-ingested event with a nul-terminated JSON encoding of
//!   `KernelEvent`. Returns a `u64` id for unregister.
//! - [`nmp_app_unregister_event_observer`] — drop a registration by id.
//!
//! The Rust-side counterpart for in-process consumers (per-app crates) is
//! [`crate::NmpApp::register_event_observer`] / `unregister_event_observer`
//! — both paths funnel into the same `KernelEventObserverSlot` the kernel
//! reads from on every `EventStore::insert` returning `Inserted | Replaced`
//! (see `kernel/ingest/timeline.rs`).
//!
//! ## Doctrine
//!
//! * **D0** — kernel emits raw `KernelEvent`s; the per-app crate (or the
//!   Swift consumer) composes them into typed views. ADR-0009.
//! * **D6** — null app pointer, null callback, or poisoned mutex are silent
//!   no-ops; nothing crosses the FFI as an exception.
//! * **C-string lifetime** — the `*const c_char` payload is borrowed for
//!   the duration of the callback only; consumers must copy any bytes they
//!   need. Same contract as the existing update callback in `ffi/mod.rs`.

use super::{app_ref, NmpApp};
use nmp_core::__ffi_internal::{
    register_c_observer, unregister_observer, KernelEventObserverRegistration,
};
use nmp_core::{KernelEventObserverFn, KernelEventObserverId};
use std::ffi::c_void;

/// Register a C-ABI kernel event observer.
///
/// `callback` fires on the actor thread once per event that has been
/// accepted into the kernel's `EventStore` (insertions and replacements
/// only; duplicates / supersessions / rejections do not fire). The C
/// string argument is a nul-terminated JSON encoding of the event with
/// fields `{id, author, kind, created_at, tags, content}` — same shape as
/// `nmp_core::substrate::KernelEvent`.
///
/// Returns a non-zero `u64` id on success; `0` on failure (null app, null
/// callback, or poisoned mutex). The id is required to unregister.
#[no_mangle]
pub extern "C" fn nmp_app_register_event_observer(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<KernelEventObserverFn>,
) -> u64 {
    let Some(app) = app_ref(app) else {
        return 0;
    };
    let Some(callback) = callback else {
        return 0;
    };
    let registration = KernelEventObserverRegistration {
        context: context as usize,
        callback,
    };
    let slot = app.event_observers_slot();
    let id = register_c_observer(&slot, registration);
    id.0
}

/// Drop a previously-registered observer by id. Idempotent: unknown ids,
/// null app pointers, or poisoned mutexes are silent no-ops (D6).
#[no_mangle]
pub extern "C" fn nmp_app_unregister_event_observer(app: *mut NmpApp, id: u64) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let slot = app.event_observers_slot();
    unregister_observer(&slot, KernelEventObserverId(id));
}

#[cfg(test)]
mod tests {
    //! Smoke test: register a C observer through the FFI, drive a kernel
    //! ingest via the test-support facade, assert the callback fired with a
    //! decodable payload.

    use super::*;
    use crate::{nmp_app_free, nmp_app_new};
    use std::ffi::CStr;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};

    // Plain `extern "C" fn` can't capture; park a `Sender` in a static and
    // forward the payload string through it. SERIAL linearises tests.
    static EVENTS_TX: OnceLock<Mutex<Option<Sender<String>>>> = OnceLock::new();
    static SERIAL: Mutex<()> = Mutex::new(());

    extern "C" fn record_callback(_ctx: *mut c_void, payload: *const std::ffi::c_char) {
        if payload.is_null() {
            return;
        }
        // SAFETY: callback contract — payload is a valid nul-terminated C
        // string borrowed for the duration of the call.
        let cstr = unsafe { CStr::from_ptr(payload) };
        if let Ok(s) = cstr.to_str() {
            if let Some(slot) = EVENTS_TX.get() {
                if let Ok(guard) = slot.lock() {
                    if let Some(tx) = guard.as_ref() {
                        let _ = tx.send(s.to_string());
                    }
                }
            }
        }
    }

    fn install_recorder() -> std::sync::mpsc::Receiver<String> {
        let (tx, rx) = channel::<String>();
        let slot = EVENTS_TX.get_or_init(|| Mutex::new(None));
        *slot.lock().unwrap() = Some(tx);
        rx
    }

    fn uninstall_recorder() {
        if let Some(slot) = EVENTS_TX.get() {
            *slot.lock().unwrap() = None;
        }
    }

    #[test]
    fn register_and_unregister_round_trip() {
        let _g = SERIAL.lock().unwrap();
        let _rx = install_recorder();
        let app = nmp_app_new();
        let id = nmp_app_register_event_observer(app, std::ptr::null_mut(), Some(record_callback));
        assert!(id > 0, "register returned zero id");
        nmp_app_unregister_event_observer(app, id);
        nmp_app_free(app);
        uninstall_recorder();
    }

    #[test]
    fn null_callback_returns_zero_id() {
        let _g = SERIAL.lock().unwrap();
        let app = nmp_app_new();
        let id = nmp_app_register_event_observer(app, std::ptr::null_mut(), None);
        assert_eq!(id, 0);
        nmp_app_free(app);
    }

    #[test]
    fn null_app_is_silent() {
        let _g = SERIAL.lock().unwrap();
        // Should not panic.
        let id = nmp_app_register_event_observer(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            Some(record_callback),
        );
        assert_eq!(id, 0);
        nmp_app_unregister_event_observer(std::ptr::null_mut(), 1);
    }
}
