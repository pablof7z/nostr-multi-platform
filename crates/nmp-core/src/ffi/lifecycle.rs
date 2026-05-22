//! T118 / G3 — app-lifecycle FFI surface.
//!
//! Three `extern "C"` symbols exported to Swift/C:
//! * [`nmp_app_lifecycle_foreground`] — shell reports `scenePhase == .active`.
//! * [`nmp_app_lifecycle_background`] — shell reports `scenePhase == .background`.
//! * [`nmp_app_set_lifecycle_callback`] — register the native observer that
//!   fans `Background → Foreground` transitions out to whatever the shell
//!   wants to react with (e.g. a sync-trigger engine). Mirrors the
//!   `capability_callback` registration pattern in `ffi/capability.rs`.
//!
//! ## Doctrine
//!
//! * **D6** — every symbol is fire-and-forget; null app, poisoned mutex,
//!   or absent observer are silent no-ops. Nothing crosses the FFI as an
//!   exception.
//! * **D7** — Swift only reports the fact of a scenePhase change; the
//!   kernel decides what each phase *means* (when to reconcile sync, how
//!   to throttle retries). No native code names the shell's sync-trigger
//!   engine — the observer callback is the seam.

use super::{app_ref, NmpApp};
use crate::actor::{ActorCommand, LifecycleObserverFn, LifecycleObserverRegistration};
use crate::kernel::LifecyclePhase;
use std::ffi::c_void;

/// Report iOS `scenePhase == .active` (or platform equivalent). Fire-and-
/// forget: the actor folds the phase into the kernel and fires the
/// registered observer on a Background→Foreground (or first-after-boot)
/// transition. Repeated `Foreground` calls debounce to no-op.
#[no_mangle]
pub extern "C" fn nmp_app_lifecycle_foreground(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let _ = app
        .tx
        .send(ActorCommand::LifecycleEvent(LifecyclePhase::Foreground));
}

/// Report iOS `scenePhase == .background` (or platform equivalent). Symmetric
/// to [`nmp_app_lifecycle_foreground`]; today no built-in consumer reacts
/// to the Background transition (the canonical shell-side sync-trigger
/// vocabulary covers only `Foreground`, `ViewOpenedWithGap`,
/// `RelayReconnected`), but the hook is surfaced so future
/// close-idle-sockets-after-grace-period policy can plug in without breaking
/// the FFI shape.
///
/// `.inactive` (the iOS interstitial state between active and background)
/// has NO FFI symbol — the shell silently no-ops on `.inactive` per the
/// PulseApp scenePhase observer. The kernel-side `LifecyclePhase::Inactive`
/// is the boot sentinel, not a phase the shell ever reports.
#[no_mangle]
pub extern "C" fn nmp_app_lifecycle_background(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let _ = app
        .tx
        .send(ActorCommand::LifecycleEvent(LifecyclePhase::Background));
}

/// Register a native handler that fires on meaningful phase transitions.
/// The callback receives a `u32` phase code:
/// * `0` ([`crate::actor::LIFECYCLE_PHASE_FOREGROUND`]) — entered foreground.
/// * `1` ([`crate::actor::LIFECYCLE_PHASE_BACKGROUND`]) — entered background.
///
/// Passing `None` unregisters. The callback executes on the actor thread;
/// it must be cheap and re-entrancy-safe (the actor releases its internal
/// mutex before invoking the callback, so re-registering inside the
/// callback is legal). Mirrors `nmp_app_set_capability_callback`.
#[no_mangle]
pub extern "C" fn nmp_app_set_lifecycle_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<LifecycleObserverFn>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Ok(mut slot) = app.lifecycle_observer.lock() else {
        return;
    };
    *slot = callback.map(|callback| LifecycleObserverRegistration {
        context: context as usize,
        callback,
    });
}

#[cfg(test)]
mod tests {
    //! End-to-end FFI smoke: register a callback, dispatch foreground +
    //! background through the FFI, assert the callback was invoked with
    //! the right phase codes. The actor thread runs in the background;
    //! tests join via an mpsc channel from the callback to the test
    //! thread.

    use super::*;
    use crate::actor::{LIFECYCLE_PHASE_BACKGROUND, LIFECYCLE_PHASE_FOREGROUND};
    use crate::ffi::nmp_app_new;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    // The callback is a plain `extern "C" fn` so it can't capture; tests
    // park the test-side mpsc Sender in a static and the callback drains
    // through it. SERIAL linearises tests so the static is owned by one
    // test at a time (the OnceLock is initialised at first access).
    static EVENTS_TX: OnceLock<Mutex<Option<Sender<u32>>>> = OnceLock::new();
    static SERIAL: Mutex<()> = Mutex::new(());

    extern "C" fn record_callback(_ctx: *mut c_void, phase: u32) {
        if let Some(slot) = EVENTS_TX.get() {
            if let Ok(guard) = slot.lock() {
                if let Some(tx) = guard.as_ref() {
                    let _ = tx.send(phase);
                }
            }
        }
    }

    fn install_recorder() -> std::sync::mpsc::Receiver<u32> {
        let (tx, rx) = channel::<u32>();
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
    fn foreground_after_boot_invokes_callback() {
        let _g = SERIAL.lock().unwrap();
        let rx = install_recorder();
        let app = nmp_app_new();
        nmp_app_set_lifecycle_callback(app, std::ptr::null_mut(), Some(record_callback));

        nmp_app_lifecycle_foreground(app);

        let phase = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("callback fired");
        assert_eq!(phase, LIFECYCLE_PHASE_FOREGROUND);

        // Cleanup: drop the recorder then the app (which joins the actor).
        nmp_app_set_lifecycle_callback(app, std::ptr::null_mut(), None);
        super::super::nmp_app_free(app);
        uninstall_recorder();
    }

    #[test]
    fn rapid_double_foreground_invokes_callback_once() {
        let _g = SERIAL.lock().unwrap();
        let rx = install_recorder();
        let app = nmp_app_new();
        nmp_app_set_lifecycle_callback(app, std::ptr::null_mut(), Some(record_callback));

        nmp_app_lifecycle_foreground(app);
        nmp_app_lifecycle_foreground(app);

        let first = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("first callback");
        assert_eq!(first, LIFECYCLE_PHASE_FOREGROUND);
        // Second dispatch must NOT yield a callback (debounced). Use a
        // short timeout; a positive wait would be a debounce-violation.
        let second = rx.recv_timeout(Duration::from_millis(300));
        assert!(
            second.is_err(),
            "second Foreground must debounce, got {second:?}",
        );

        nmp_app_set_lifecycle_callback(app, std::ptr::null_mut(), None);
        super::super::nmp_app_free(app);
        uninstall_recorder();
    }

    #[test]
    fn foreground_then_background_then_foreground_swipe() {
        let _g = SERIAL.lock().unwrap();
        let rx = install_recorder();
        let app = nmp_app_new();
        nmp_app_set_lifecycle_callback(app, std::ptr::null_mut(), Some(record_callback));

        nmp_app_lifecycle_foreground(app);
        nmp_app_lifecycle_background(app);
        nmp_app_lifecycle_foreground(app);

        let p1 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let p2 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let p3 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(p1, LIFECYCLE_PHASE_FOREGROUND);
        assert_eq!(p2, LIFECYCLE_PHASE_BACKGROUND);
        assert_eq!(p3, LIFECYCLE_PHASE_FOREGROUND);
        // No fourth event.
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());

        nmp_app_set_lifecycle_callback(app, std::ptr::null_mut(), None);
        super::super::nmp_app_free(app);
        uninstall_recorder();
    }
}
