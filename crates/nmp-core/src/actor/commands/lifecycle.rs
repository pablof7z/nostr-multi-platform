//! T118 / G3 — lifecycle command handler.
//!
//! Folds an [`ActorCommand::LifecycleEvent`] into the kernel's phase state
//! and, on a meaningful transition (per `LifecyclePhase::transition_from`'s
//! debounce rules), invokes the registered [`LifecycleObserver`] so a
//! consumer can fan the transition out to its own machinery (typically a
//! shell-side sync-trigger engine on a foreground transition).
//!
//! ## Doctrine
//!
//! * **D0** — the kernel never names any shell-side trigger-engine types;
//!   the observer callback decouples the trigger fan-out. nmp-core stays
//!   free of policy-crate deps (would be a cycle — any such crate consumes
//!   nmp-core's substrate).
//! * **D6** — the observer is invoked best-effort. A poisoned mutex or
//!   absent registration is a silent no-op; nothing crosses the FFI as an
//!   exception.
//! * **D7** — the iOS shell reports the *fact* of a scenePhase change; the
//!   kernel decides what it *means*. The shell never calls into the
//!   trigger engine directly; every consequence threads through here.
//! * **Idempotence** — `kernel.set_lifecycle_phase` returns `None` for
//!   no-op transitions (rapid scene oscillation, `Foreground→Foreground`);
//!   the observer fires only on meaningful state changes.

use crate::kernel::{Kernel, LifecyclePhase, LifecycleTransition};
use std::sync::{Arc, Mutex};

/// Lifecycle observer C-ABI shape. Mirrors the `capability_callback`
/// pattern: `extern "C"` so it can be plugged in from Swift, and stores a
/// caller-opaque context pointer for state. The phase is passed as a `u32`
/// discriminant (0=Foreground, 1=Background) so the wire format is
/// language-agnostic.
pub type LifecycleObserverFn = extern "C" fn(*mut std::ffi::c_void, u32);

/// Phase wire discriminants. Public for FFI consumers (the Swift bridge or
/// integration tests via the test-support facade).
pub const LIFECYCLE_PHASE_FOREGROUND: u32 = 0;
pub const LIFECYCLE_PHASE_BACKGROUND: u32 = 1;

/// Registered native handler + caller context. `Copy` so it can be cloned
/// out from under the mutex lock without holding it across the FFI call
/// (avoids reentrancy if the consumer were to immediately re-register).
#[derive(Clone, Copy)]
pub struct LifecycleObserverRegistration {
    /// Caller-opaque context pointer, as registered. `usize` storage
    /// (rather than `*mut c_void`) is the same dodge `capability.rs` uses
    /// for `Send` / `Sync` — raw pointers aren't either; the callsite
    /// re-casts on invocation.
    pub context: usize,
    pub callback: LifecycleObserverFn,
}

/// Shared slot. The FFI surface (`ffi/lifecycle.rs`) holds one clone for
/// registration; the actor thread holds another for invocation. `Mutex`
/// ensures registration and invocation never tear.
pub type LifecycleObserverSlot = Arc<Mutex<Option<LifecycleObserverRegistration>>>;

/// Construct an empty slot. Called once in `nmp_app_new`.
pub fn new_observer_slot() -> LifecycleObserverSlot {
    Arc::new(Mutex::new(None))
}

/// Drive a phase update through the kernel and fire the observer on a
/// meaningful transition. Returns the transition verdict for the dispatch
/// reducer's tests and bookkeeping; the observer side-effect already
/// happened by the time this returns.
pub(crate) fn handle_lifecycle_event(
    kernel: &mut Kernel,
    observer: &LifecycleObserverSlot,
    phase: LifecyclePhase,
) -> Option<LifecycleTransition> {
    let transition = kernel.set_lifecycle_phase(phase)?;
    // Snapshot the registration under the lock, then release it before
    // invoking the callback. The callback may legitimately re-enter
    // `nmp_app_set_lifecycle_callback`; holding the lock across that
    // re-entry would deadlock. `Copy` registration makes the snapshot
    // pointer-cheap.
    let snapshot = observer.lock().ok().and_then(|guard| *guard);
    if let Some(registration) = snapshot {
        let phase_code = match transition {
            LifecycleTransition::EnteredForeground => LIFECYCLE_PHASE_FOREGROUND,
            LifecycleTransition::EnteredBackground => LIFECYCLE_PHASE_BACKGROUND,
        };
        // UB guard: the foreign callback may panic / raise; an unwind
        // across the C ABI boundary is undefined behaviour.
        let _ = crate::ffi_guard::guard_ffi_callback("lifecycle observer", || {
            (registration.callback)(registration.context as *mut std::ffi::c_void, phase_code);
        });
    }
    Some(transition)
}

#[cfg(test)]
mod tests {
    //! Tests use static counters because `LifecycleObserverFn` is a plain
    //! `extern "C" fn` (no captures). `SERIAL` linearises test cases so the
    //! statics see one test's events at a time — same pattern as
    //! `ffi/capability.rs` tests.

    use super::*;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use std::sync::atomic::{AtomicU32, Ordering};

    static CALLS: AtomicU32 = AtomicU32::new(0);
    static LAST_PHASE: AtomicU32 = AtomicU32::new(u32::MAX);
    static SERIAL: Mutex<()> = Mutex::new(());

    extern "C" fn observer_shim(_ctx: *mut std::ffi::c_void, phase: u32) {
        CALLS.fetch_add(1, Ordering::SeqCst);
        LAST_PHASE.store(phase, Ordering::SeqCst);
    }

    fn fixture() -> (Kernel, LifecycleObserverSlot) {
        CALLS.store(0, Ordering::SeqCst);
        LAST_PHASE.store(u32::MAX, Ordering::SeqCst);
        let slot = new_observer_slot();
        *slot.lock().unwrap() = Some(LifecycleObserverRegistration {
            context: 0,
            callback: observer_shim,
        });
        (Kernel::new(DEFAULT_VISIBLE_LIMIT), slot)
    }

    #[test]
    fn boot_to_foreground_fires_observer_once() {
        let _g = SERIAL.lock().unwrap();
        let (mut kernel, slot) = fixture();
        let t = handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Foreground);
        assert_eq!(t, Some(LifecycleTransition::EnteredForeground));
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(
            LAST_PHASE.load(Ordering::SeqCst),
            LIFECYCLE_PHASE_FOREGROUND
        );
    }

    #[test]
    fn rapid_double_foreground_only_fires_once() {
        let _g = SERIAL.lock().unwrap();
        let (mut kernel, slot) = fixture();
        let t1 = handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Foreground);
        let t2 = handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Foreground);
        assert_eq!(t1, Some(LifecycleTransition::EnteredForeground));
        assert_eq!(t2, None, "second Foreground must debounce");
        assert_eq!(
            CALLS.load(Ordering::SeqCst),
            1,
            "observer fires only on the first transition",
        );
    }

    #[test]
    fn background_then_foreground_swipe_fires_each_once() {
        let _g = SERIAL.lock().unwrap();
        let (mut kernel, slot) = fixture();
        handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Foreground);
        let t_bg = handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Background);
        let t_fg = handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Foreground);
        assert_eq!(t_bg, Some(LifecycleTransition::EnteredBackground));
        assert_eq!(t_fg, Some(LifecycleTransition::EnteredForeground));
        assert_eq!(CALLS.load(Ordering::SeqCst), 3);
        assert_eq!(
            LAST_PHASE.load(Ordering::SeqCst),
            LIFECYCLE_PHASE_FOREGROUND
        );
    }

    #[test]
    fn observer_absent_is_silent_noop() {
        let _g = SERIAL.lock().unwrap();
        let (mut kernel, slot) = fixture();
        *slot.lock().unwrap() = None;
        let t = handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Foreground);
        assert_eq!(t, Some(LifecycleTransition::EnteredForeground));
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    }
}
