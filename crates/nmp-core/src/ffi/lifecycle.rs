//! T118 / G3 — app-lifecycle FFI surface.
//!
//! Four `extern "C"` symbols exported to Swift/C:
//! * [`nmp_app_lifecycle_foreground`] — shell reports `scenePhase == .active`.
//! * [`nmp_app_lifecycle_background`] — shell reports `scenePhase == .background`.
//! * [`nmp_app_set_lifecycle_callback`] — register the native observer that
//!   fans `Background → Foreground` transitions out to whatever the shell
//!   wants to react with (e.g. a sync-trigger engine). Mirrors the
//!   `capability_callback` registration pattern in `ffi/capability.rs`.
//! * [`nmp_app_is_alive`] — the actor-liveness probe. Returns `1` when the
//!   actor thread is still running, `0` when it has terminated (panic, drop,
//!   or pre-`nmp_app_start`-but-handle-already-finished — all collapse to
//!   "dead from the host's perspective"). Pairs with the
//!   [`crate::update_envelope::UpdateEnvelope::Panic`] frame (D7): the panic
//!   frame is the *push* death signal on the update channel; this probe is
//!   the *pull* sibling, queryable on demand (e.g. on
//!   `applicationWillEnterForeground` after the host has been backgrounded
//!   long enough for the panic frame to have arrived while the host wasn't
//!   processing the channel).
//!
//! ## Doctrine
//!
//! * **D6** — every symbol is fire-and-forget; null app, poisoned mutex,
//!   or absent observer are silent no-ops. Nothing crosses the FFI as an
//!   exception. `nmp_app_is_alive` returns `0` for null / poisoned / dead;
//!   the host treats every non-`1` response as "kernel gone".
//! * **D7** — Swift only reports the fact of a scenePhase change; the
//!   kernel decides what each phase *means* (when to reconcile sync, how
//!   to throttle retries). No native code names the shell's sync-trigger
//!   engine — the observer callback is the seam. The liveness probe is
//!   strictly observability — it does not influence kernel behaviour, only
//!   surfaces it to the host.

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
    app.send_cmd(ActorCommand::LifecycleEvent(LifecyclePhase::Foreground));
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
/// `PulseApp` scenePhase observer. The kernel-side `LifecyclePhase::Inactive`
/// is the boot sentinel, not a phase the shell ever reports.
#[no_mangle]
pub extern "C" fn nmp_app_lifecycle_background(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    app.send_cmd(ActorCommand::LifecycleEvent(LifecyclePhase::Background));
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

/// Actor-liveness probe (D7 pull-side sibling of the
/// [`crate::update_envelope::UpdateEnvelope::Panic`] push-side frame).
///
/// Returns `1` when the actor `JoinHandle` is still running, `0` otherwise:
/// * `app == NULL` → `0` (no kernel to be alive)
/// * `actor` mutex poisoned → `0` (kernel state is irrecoverable, treat as dead)
/// * `actor` slot is `None` (already joined by [`Drop`]) → `0`
/// * `JoinHandle::is_finished()` returns `true` → `0`
/// * otherwise → `1`
///
/// `is_finished()` flips synchronously when the actor thread exits — both the
/// clean `Shutdown` path AND the panic path go through the supervisor closure
/// in [`super::nmp_app_new`], so this probe agrees with the panic frame either
/// way (the panic frame fires, the supervisor closure unwinds, the thread
/// exits, the handle becomes finished). The probe is therefore the
/// pull-equivalent of the push frame: a host that missed the panic frame
/// (e.g. the app was backgrounded while it landed on the channel, then the
/// listener thread drained and exited before the host re-attached) can call
/// this on resume and learn the same fact.
///
/// # Safety
/// `app` must be a valid non-null pointer from [`super::nmp_app_new`], or
/// null (null is a silent `0`). The function performs no foreign callback
/// and acquires only the `actor` mutex; no panic can cross the C ABI
/// boundary.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_is_alive(app: *mut NmpApp) -> u8 {
    let Some(app) = app_ref(app) else {
        return 0;
    };
    // D6 — a poisoned mutex on the actor slot is "kernel state is broken";
    // collapse to dead rather than panic across the FFI seam.
    let Ok(guard) = app.actor.lock() else {
        return 0;
    };
    match guard.as_ref() {
        Some(handle) if !handle.is_finished() => 1,
        // `None` is the post-`Drop` (or never-started) state; `Some` with
        // `is_finished()` is the post-panic / post-`Shutdown` state. Both are
        // "the actor will no longer service commands" — the host must surface
        // a fatal error.
        _ => 0,
    }
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

    // ── nmp_app_is_alive probe contract (D7 pull-side) ─────────────────────

    /// Null app pointer → `0`. Mirrors every other `app_ref`-gated symbol's
    /// silent-no-op contract.
    #[test]
    fn is_alive_null_app_returns_zero() {
        assert_eq!(super::nmp_app_is_alive(std::ptr::null_mut()), 0);
    }

    /// A freshly-allocated `NmpApp` whose actor thread has been spawned (by
    /// `nmp_app_new`) reports alive. The actor parks on the command channel
    /// `recv()` until a `Start` arrives, so this exercises the
    /// "spawned but idle" path — exactly the state the iOS app sits in
    /// between `nmp_app_new` and the first `nmp_app_start` call.
    #[test]
    fn is_alive_after_new_returns_one() {
        let _g = SERIAL.lock().unwrap();
        let app = nmp_app_new();
        assert_eq!(super::nmp_app_is_alive(app), 1, "actor should be alive");
        super::super::nmp_app_free(app);
    }

    /// After the actor receives `Shutdown` (the clean-exit path the `Drop`
    /// impl issues), the thread exits and `is_finished()` flips to `true`,
    /// so the probe returns `0`. This proves the probe agrees with the
    /// panic path too: both the clean-exit and panic supervisors end the
    /// thread, and `is_finished()` is set in both cases.
    ///
    /// `nmp_app_free` joins the actor synchronously, so by the time the
    /// helper returns the handle is `None` in the slot — the probe still
    /// returns `0` (covered by the `None` arm of the match). We exercise
    /// the `is_finished()` arm by sending `Shutdown` ourselves through the
    /// internal accessor and joining out-of-band before `free`.
    #[test]
    fn is_alive_returns_zero_after_actor_shutdown() {
        use crate::actor::ActorCommand;
        let _g = SERIAL.lock().unwrap();
        let app = nmp_app_new();
        // Push `Shutdown` straight onto the actor's command channel
        // (mirrors what `Drop` does, but without joining yet).
        // SAFETY: `app` is a live pointer from `nmp_app_new` above.
        let app_ref = unsafe { &*app };
        let _ = app_ref.actor_sender().send(ActorCommand::Shutdown);
        // Spin briefly waiting for the actor to dequeue and exit. The
        // command channel is `recv()`-blocked so the dequeue is immediate
        // once the message lands; a short timeout absorbs scheduling
        // jitter without serialising the test for seconds.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if super::nmp_app_is_alive(app) == 0 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "actor did not exit within 2s of Shutdown"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        super::super::nmp_app_free(app);
    }
}
