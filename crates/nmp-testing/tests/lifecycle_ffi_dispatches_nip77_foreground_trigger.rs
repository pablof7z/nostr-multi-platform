//! T118 / G3 integration test — iOS scenePhase FFI fans
//! [`TriggerEvent::Foreground`] through to a `nmp_nip77::TriggerEngine`.
//!
//! The kernel cannot depend on `nmp-nip77` (would be a dep cycle —
//! `nmp-nip77` already consumes `nmp-core::store`). The lifecycle FFI
//! therefore exposes an `extern "C" fn` observer seam: a consumer
//! registers a callback that fires on meaningful phase transitions, and
//! the consumer is what dispatches `TriggerEvent::Foreground` into its
//! own `TriggerEngine`.
//!
//! This test lives in `nmp-testing` because that crate is the first place
//! both `nmp-core` (for the FFI) and `nmp-nip77` (for `TriggerEngine`) are
//! linked at once, so the end-to-end contract is observable here and
//! nowhere upstream.
//!
//! ## What the test proves
//!
//! 1. **FFI → observer wiring.** `nmp_app_lifecycle_foreground` causes the
//!    registered callback to fire with `LIFECYCLE_PHASE_FOREGROUND`.
//! 2. **Observer → TriggerEngine bridge.** The callback dispatches
//!    `TriggerEvent::Foreground` into a `TriggerEngine` populated with two
//!    open `(filter, relay)` pairs; the engine returns work for *both*
//!    pairs.
//! 3. **Idempotence.** A second `nmp_app_lifecycle_foreground` while still
//!    foregrounded is a no-op — observer fires once, reconciler is
//!    invoked once.
//! 4. **Background→Foreground swipe.** A rapid `bg → fg` oscillation
//!    yields one `EnteredBackground` callback and one `EnteredForeground`
//!    callback; the reconcile work fires only on the `EnteredForeground`
//!    half.
//!
//! ## Mechanics
//!
//! `LifecycleObserverFn` is a bare `extern "C" fn(*mut c_void, u32)` — no
//! captures. The callback drains through a `&'static Mutex<Sender<u32>>`
//! installed by the test before the FFI is wired and uninstalled after.
//! A module-level `SERIAL` mutex linearises tests so the static slot has
//! one writer at a time. Same pattern as `ffi/lifecycle.rs::tests`.

use std::ffi::c_void;
use std::ptr;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use nmp_core::{
    nmp_app_free, nmp_app_lifecycle_background, nmp_app_lifecycle_foreground, nmp_app_new,
    nmp_app_set_lifecycle_callback, LifecycleObserverFn, LIFECYCLE_PHASE_BACKGROUND,
    LIFECYCLE_PHASE_FOREGROUND,
};
use nmp_nip77::{ReconcileWork, TriggerEngine, TriggerEvent};

// `extern "C" fn` cannot capture state; the callback drains a `u32` phase
// code through this static slot. Tests linearise via `SERIAL`.
static EVENTS_TX: OnceLock<Mutex<Option<Sender<u32>>>> = OnceLock::new();
static SERIAL: Mutex<()> = Mutex::new(());

extern "C" fn lifecycle_recorder(_ctx: *mut c_void, phase: u32) {
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

/// Build a TriggerEngine pre-populated with two open `(filter, relay)`
/// pairs distributed across two relays — gives the assertion something
/// non-trivial to verify (a `Foreground` trigger fans out across *every*
/// open pair, not just one).
fn populated_trigger_engine() -> TriggerEngine {
    let mut eng = TriggerEngine::new();
    let f1 = [0xA1u8; 32];
    let f2 = [0xB2u8; 32];
    eng.register(f1, "wss://relay-1.example/");
    eng.register(f2, "wss://relay-2.example/");
    eng
}

/// Bridge the observer phase code into a `TriggerEvent`. This is the
/// integration the iOS shell (or its Rust-side equivalent) is expected
/// to write — the kernel exposes the *fact* of the transition (D7), the
/// consumer maps it to a domain event.
fn phase_to_trigger(phase: u32) -> Option<TriggerEvent> {
    match phase {
        p if p == LIFECYCLE_PHASE_FOREGROUND => Some(TriggerEvent::Foreground),
        _ => None,
    }
}

#[test]
fn foreground_ffi_fires_observer_and_dispatches_trigger_to_nip77() {
    let _guard = SERIAL.lock().unwrap();
    let rx = install_recorder();
    let app = nmp_app_new();

    // Register the observer that bridges the kernel transition to a
    // domain trigger. This is the seam Pulse will install via the same
    // FFI symbol from Swift.
    let cb: LifecycleObserverFn = lifecycle_recorder;
    nmp_app_set_lifecycle_callback(app, ptr::null_mut(), Some(cb));

    // Build a TriggerEngine — production code would keep one in a
    // long-lived state; the test stands one up freshly because the
    // assertion is about the fan-out shape, not engine ownership.
    let engine = populated_trigger_engine();

    // Drive the FFI — the actor folds the phase, the observer fires.
    nmp_app_lifecycle_foreground(app);

    let phase = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("observer fired");
    assert_eq!(
        phase, LIFECYCLE_PHASE_FOREGROUND,
        "first FFI call after boot must be EnteredForeground",
    );

    // Bridge to the trigger. The reconcile work covers ALL open pairs —
    // the whole point of the Foreground trigger (vs ViewOpenedWithGap).
    let trigger = phase_to_trigger(phase).expect("Foreground maps to a trigger");
    let work: Vec<ReconcileWork> = engine.on_event(trigger);
    assert_eq!(
        work.len(),
        2,
        "Foreground must reconcile every (filter, relay) pair, got {work:?}",
    );

    nmp_app_set_lifecycle_callback(app, ptr::null_mut(), None);
    nmp_app_free(app);
    uninstall_recorder();
}

#[test]
fn rapid_double_foreground_fires_trigger_only_once() {
    let _guard = SERIAL.lock().unwrap();
    let rx = install_recorder();
    let app = nmp_app_new();
    nmp_app_set_lifecycle_callback(app, ptr::null_mut(), Some(lifecycle_recorder));

    nmp_app_lifecycle_foreground(app);
    nmp_app_lifecycle_foreground(app);

    let first = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first observer call");
    assert_eq!(first, LIFECYCLE_PHASE_FOREGROUND);

    // Second `Foreground` while still foregrounded must debounce — no
    // second observer event. A positive recv here is a debounce
    // violation (would cause `TriggerEngine::on_event(Foreground)` to
    // fire twice, doubling network work on a back-foreground swipe).
    let second = rx.recv_timeout(Duration::from_millis(300));
    assert!(
        second.is_err(),
        "second Foreground must debounce; got {second:?}",
    );

    nmp_app_set_lifecycle_callback(app, ptr::null_mut(), None);
    nmp_app_free(app);
    uninstall_recorder();
}

#[test]
fn background_then_foreground_swipe_yields_single_trigger_pair() {
    let _guard = SERIAL.lock().unwrap();
    let rx = install_recorder();
    let app = nmp_app_new();
    nmp_app_set_lifecycle_callback(app, ptr::null_mut(), Some(lifecycle_recorder));

    let engine = populated_trigger_engine();

    // Boot into foreground (so Background→Foreground is the
    // trigger-bearing transition; without this, the swipe-back would be
    // the FIRST foreground event and tests the boot path instead).
    nmp_app_lifecycle_foreground(app);
    let p0 = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(p0, LIFECYCLE_PHASE_FOREGROUND);

    // Swipe out then immediately back in — rapid bg/fg oscillation, the
    // exact scenePhase pattern the iOS app-switcher produces.
    nmp_app_lifecycle_background(app);
    nmp_app_lifecycle_foreground(app);

    let p_bg = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let p_fg = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(p_bg, LIFECYCLE_PHASE_BACKGROUND);
    assert_eq!(p_fg, LIFECYCLE_PHASE_FOREGROUND);

    // Exactly two events — the swipe MUST NOT yield a third (e.g. a
    // duplicate Foreground from the in-between Inactive).
    let extra = rx.recv_timeout(Duration::from_millis(200));
    assert!(extra.is_err(), "no extra observer event; got {extra:?}");

    // Final reconcile work fires ONCE for the swipe-back Foreground.
    let trigger = phase_to_trigger(p_fg).expect("Foreground maps to a trigger");
    let work: Vec<ReconcileWork> = engine.on_event(trigger);
    assert_eq!(work.len(), 2, "single Foreground reconciles every open pair");

    nmp_app_set_lifecycle_callback(app, ptr::null_mut(), None);
    nmp_app_free(app);
    uninstall_recorder();
}
