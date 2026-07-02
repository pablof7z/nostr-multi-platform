//! T118 / G3 — lifecycle command handler.
//!
//! Folds an [`ActorCommand::LifecycleEvent`] into the kernel's phase state
//! and, on a meaningful transition (per `LifecyclePhase::transition_from`'s
//! debounce rules), invokes the registered native lifecycle observer so a
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
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// Phase wire discriminants. Public for UniFFI consumers (or integration
/// tests via the test-support facade) that need to interpret the phase code
/// passed to the native observer.
pub const LIFECYCLE_PHASE_FOREGROUND: u32 = 0;
pub const LIFECYCLE_PHASE_BACKGROUND: u32 = 1;

/// Rust-native lifecycle observer used by the UniFFI surface. Receives the
/// phase wire discriminant for the transition.
pub type NativeLifecycleObserver = Arc<dyn Fn(u32) + Send + Sync + 'static>;

struct LifecycleObserverGateInner {
    observer: Option<NativeLifecycleObserver>,
    in_flight: u32,
}

/// Quiescence-safe lifecycle observer slot.
///
/// Set/replace/clear waits for any callback already copied from the slot to
/// finish before returning. This lets UniFFI hosts release the previous
/// callback state immediately after unregistering it.
pub struct LifecycleObserverGate {
    inner: Mutex<LifecycleObserverGateInner>,
    drained: Condvar,
}

impl LifecycleObserverGate {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(LifecycleObserverGateInner {
                observer: None,
                in_flight: 0,
            }),
            drained: Condvar::new(),
        }
    }

    pub fn set_native_observer(&self, observer: Option<NativeLifecycleObserver>) {
        let mut guard = self.lock_inner();
        guard.observer = observer;
        drop(self.wait_drained(guard));
    }

    pub fn clear(&self) {
        let mut guard = self.lock_inner();
        guard.observer = None;
        drop(self.wait_drained(guard));
    }

    fn begin_invocation(&self) -> Option<(NativeLifecycleObserver, LifecycleInvocation<'_>)> {
        let mut guard = self.lock_inner();
        let observer = Arc::clone(guard.observer.as_ref()?);
        guard.in_flight = guard.in_flight.saturating_add(1);
        Some((observer, LifecycleInvocation { gate: self }))
    }

    fn finish_invocation(&self) {
        let mut guard = self.lock_inner();
        guard.in_flight = guard.in_flight.saturating_sub(1);
        if guard.in_flight == 0 {
            self.drained.notify_all();
        }
    }

    fn lock_inner(&self) -> MutexGuard<'_, LifecycleObserverGateInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait_drained<'a>(
        &'a self,
        guard: MutexGuard<'a, LifecycleObserverGateInner>,
    ) -> MutexGuard<'a, LifecycleObserverGateInner> {
        self.drained
            .wait_while(guard, |inner| inner.in_flight > 0)
            .unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for LifecycleObserverGate {
    fn default() -> Self {
        Self::new()
    }
}

struct LifecycleInvocation<'a> {
    gate: &'a LifecycleObserverGate,
}

impl Drop for LifecycleInvocation<'_> {
    fn drop(&mut self) {
        self.gate.finish_invocation();
    }
}

/// Shared slot. The UniFFI surface holds one clone for registration; the
/// actor thread holds another for invocation.
pub type LifecycleObserverSlot = Arc<LifecycleObserverGate>;

/// Construct an empty slot. Called once in `nmp_app_new`.
pub fn new_observer_slot() -> LifecycleObserverSlot {
    Arc::new(LifecycleObserverGate::new())
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
    if let Some((observer, _invocation)) = observer.begin_invocation() {
        let phase_code = match transition {
            LifecycleTransition::EnteredForeground => LIFECYCLE_PHASE_FOREGROUND,
            LifecycleTransition::EnteredBackground => LIFECYCLE_PHASE_BACKGROUND,
        };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            observer(phase_code);
        }));
    }
    Some(transition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use std::sync::atomic::{AtomicU32, Ordering};

    static CALLS: AtomicU32 = AtomicU32::new(0);
    static LAST_PHASE: AtomicU32 = AtomicU32::new(u32::MAX);
    static SERIAL: Mutex<()> = Mutex::new(());

    fn fixture() -> (Kernel, LifecycleObserverSlot) {
        CALLS.store(0, Ordering::SeqCst);
        LAST_PHASE.store(u32::MAX, Ordering::SeqCst);
        let slot = new_observer_slot();
        slot.set_native_observer(Some(Arc::new(|phase: u32| {
            CALLS.fetch_add(1, Ordering::SeqCst);
            LAST_PHASE.store(phase, Ordering::SeqCst);
        })));
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
        slot.clear();
        let t = handle_lifecycle_event(&mut kernel, &slot, LifecyclePhase::Foreground);
        assert_eq!(t, Some(LifecycleTransition::EnteredForeground));
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn clear_waits_for_in_flight_lifecycle_observer() {
        use std::sync::mpsc;
        use std::time::Duration;

        let _g = SERIAL.lock().unwrap();
        let slot = new_observer_slot();
        let (clear_started_tx, clear_started_rx) = mpsc::sync_channel::<()>(1);
        let (clear_done_tx, clear_done_rx) = mpsc::sync_channel::<()>(1);

        slot.set_native_observer(Some(Arc::new(move |_phase| {})));
        let (_snapshot, invocation) = slot.begin_invocation().expect("observer registered");

        let slot_for_clear = Arc::clone(&slot);
        let clear = std::thread::spawn(move || {
            clear_started_tx.send(()).unwrap();
            slot_for_clear.clear();
            clear_done_tx.send(()).unwrap();
        });
        clear_started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("clear started");
        assert!(
            clear_done_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "clear returned while lifecycle callback was still in-flight",
        );

        drop(invocation);
        clear.join().unwrap();
        clear_done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("clear returns after callback drains");
    }
}
