//! T118 / G3 — lifecycle command handler + quiescence-safe observer gate.
//!
//! Folds an [`ActorCommand::LifecycleEvent`] into the kernel's phase state
//! and, on a meaningful transition (per `LifecyclePhase::transition_from`'s
//! debounce rules), invokes the registered lifecycle observer so a consumer
//! can fan the transition out to its own machinery (typically a shell-side
//! sync-trigger engine on a foreground transition).
//!
//! ## Quiescence gate (M14-C-tail / #2429)
//!
//! The observer slot is a [`LifecycleObserverGate`] — an `in_flight` counter +
//! [`Condvar`] drain, mirroring [`crate::capability_socket::CapabilityCallbackGate`]
//! and `nmp-native-runtime`'s `UpdateListenerGate`. After `set_registration` /
//! `set_native_observer` / `clear` returns, the previous handler is guaranteed
//! to be neither registered nor mid-invocation, so a host (C-ABI context **or**
//! a UniFFI `Box<dyn LifecycleSink>` ARC) can be released the instant the setter
//! returns without use-after-free.
//!
//! Two registration paths share the same gate (last-writer-wins, exactly like
//! the capability socket):
//! * **C-ABI path** (`set_registration`) — a `LifecycleObserverRegistration`
//!   (C function pointer + context) for `nmp-ffi`.
//! * **Rust-native path** (`set_native_observer`) — a `NativeLifecycleObserver`
//!   closure for `nmp-uniffi`'s `LifecycleSink`.
//!
//! ## Doctrine
//!
//! * **D0** — the kernel never names any shell-side trigger-engine types;
//!   the observer callback decouples the trigger fan-out. nmp-core stays
//!   free of policy-crate deps (would be a cycle — any such crate consumes
//!   nmp-core's substrate).
//! * **D6** — the observer is invoked best-effort. A poisoned mutex is
//!   recovered (never a silent gate stall); an absent registration is a
//!   no-op; nothing crosses the FFI as an exception.
//! * **D7** — the iOS shell reports the *fact* of a scenePhase change; the
//!   kernel decides what it *means*. The shell never calls into the
//!   trigger engine directly; every consequence threads through here.
//! * **Idempotence** — `kernel.set_lifecycle_phase` returns `None` for
//!   no-op transitions (rapid scene oscillation, `Foreground→Foreground`);
//!   the observer fires only on meaningful state changes.

use crate::kernel::{Kernel, LifecyclePhase, LifecycleTransition};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

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
/// out from under the gate lock without holding it across the FFI call
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

/// Rust-native lifecycle observer (UniFFI path, M14-C-tail). Receives the
/// phase wire discriminant (0=Foreground, 1=Background). Used by the UniFFI
/// surface's `LifecycleSink` to avoid the C-ABI `extern "C"` trampoline.
///
/// The closure is called with the phase code copied out (no Rust lock held
/// across the call), exactly mirroring the C-ABI quiescence contract.
pub type NativeLifecycleObserver = Arc<dyn Fn(u32) + Send + Sync + 'static>;

/// Discriminates which registration path is active.
enum LifecycleHandler {
    /// C-ABI path (nmp-ffi).
    CFfi(LifecycleObserverRegistration),
    /// Rust-native path (nmp-uniffi).
    Native(NativeLifecycleObserver),
}

/// Mutable state for the lifecycle-observer quiescence gate.
///
/// `in_flight > 0` only while the actor thread is actively invoking a handler
/// copied from `handler`. Set/replace/clear waits for this counter to drain
/// before returning so hosts can release callback contexts immediately after
/// the setter returns.
struct LifecycleObserverGateInner {
    handler: Option<LifecycleHandler>,
    in_flight: u32,
}

/// Quiescence-safe slot for the lifecycle observer registration.
///
/// Mirrors the capability-callback / update-listener contract: after replacing
/// or clearing the registration, the previous handler is neither registered nor
/// mid-invocation. Native bridges may free or release the previous context (a
/// C `void*` or a UniFFI `Box<dyn LifecycleSink>` ARC) after the setter returns.
pub struct LifecycleObserverGate {
    inner: Mutex<LifecycleObserverGateInner>,
    drained: Condvar,
}

impl LifecycleObserverGate {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(LifecycleObserverGateInner {
                handler: None,
                in_flight: 0,
            }),
            drained: Condvar::new(),
        }
    }

    /// Replace or clear the C-ABI observer registration, then wait for all
    /// in-flight invocations to complete before returning.
    ///
    /// Clears any active Rust-native observer (last-writer-wins: a C-ABI
    /// registration replaces the UniFFI observer and vice versa).
    ///
    /// Re-entrancy: a lifecycle callback must not call the setter for the same
    /// slot from inside the callback, because the setter waits for that
    /// callback to finish (it would deadlock against its own drain).
    pub fn set_registration(&self, registration: Option<LifecycleObserverRegistration>) {
        let mut guard = self.lock_inner();
        guard.handler = registration.map(LifecycleHandler::CFfi);
        drop(self.wait_drained(guard));
    }

    /// Replace or clear the Rust-native lifecycle observer, then wait for all
    /// in-flight invocations to complete before returning.
    ///
    /// Clears any active C-ABI registration (last-writer-wins). Used by the
    /// UniFFI surface (M14-C-tail) to register a `LifecycleSink` without a C
    /// function-pointer trampoline. Same quiescence contract as
    /// [`Self::set_registration`].
    pub fn set_native_observer(&self, observer: Option<NativeLifecycleObserver>) {
        let mut guard = self.lock_inner();
        guard.handler = observer.map(LifecycleHandler::Native);
        drop(self.wait_drained(guard));
    }

    /// Clear any registration (either path) and drain in-flight invocations.
    pub fn clear(&self) {
        let mut guard = self.lock_inner();
        guard.handler = None;
        drop(self.wait_drained(guard));
    }

    #[must_use]
    pub fn is_registered(&self) -> bool {
        self.lock_inner().handler.is_some()
    }

    fn begin_invocation(&self) -> Option<(LifecycleHandleSnapshot, LifecycleInvocation<'_>)> {
        let mut guard = self.lock_inner();
        let snapshot = match guard.handler.as_ref()? {
            LifecycleHandler::CFfi(r) => LifecycleHandleSnapshot::CFfi(*r),
            LifecycleHandler::Native(f) => LifecycleHandleSnapshot::Native(Arc::clone(f)),
        };
        guard.in_flight = guard.in_flight.saturating_add(1);
        Some((snapshot, LifecycleInvocation { gate: self }))
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

/// Snapshot of the active handler, taken while holding the gate lock and before
/// `in_flight` is incremented. Decouples the actual dispatch from the lock.
enum LifecycleHandleSnapshot {
    CFfi(LifecycleObserverRegistration),
    Native(NativeLifecycleObserver),
}

/// RAII guard: increments `in_flight` on [`LifecycleObserverGate::begin_invocation`],
/// decrements + notifies on drop. Guarantees the in-flight count is balanced
/// even if the dispatch panics.
struct LifecycleInvocation<'a> {
    gate: &'a LifecycleObserverGate,
}

impl Drop for LifecycleInvocation<'_> {
    fn drop(&mut self) {
        self.gate.finish_invocation();
    }
}

/// Shared slot. The FFI surface (`ffi/lifecycle.rs`) and the UniFFI surface
/// hold one clone for registration; the actor thread holds another for
/// invocation. The gate's `in_flight` + Condvar guarantee registration and
/// invocation never tear and that teardown drains.
pub type LifecycleObserverSlot = Arc<LifecycleObserverGate>;

/// Construct an empty slot. Called once in `nmp_app_new`.
#[must_use]
pub fn new_observer_slot() -> LifecycleObserverSlot {
    Arc::new(LifecycleObserverGate::new())
}

/// Drive a phase update through the kernel and fire the observer on a
/// meaningful transition. Returns the transition verdict for the dispatch
/// reducer's tests and bookkeeping; the observer side-effect already
/// happened by the time this returns.
///
/// The handler is snapshotted (and `in_flight` incremented) under the gate
/// lock, then the lock is released before the foreign call — so a concurrent
/// `set_*`/`clear` cannot tear the registration and a re-entrant setter would
/// only block (never UAF). The [`LifecycleInvocation`] guard decrements
/// `in_flight` and wakes any waiting setter when the call returns.
pub(crate) fn handle_lifecycle_event(
    kernel: &mut Kernel,
    observer: &LifecycleObserverSlot,
    phase: LifecyclePhase,
) -> Option<LifecycleTransition> {
    let transition = kernel.set_lifecycle_phase(phase)?;
    let phase_code = match transition {
        LifecycleTransition::EnteredForeground => LIFECYCLE_PHASE_FOREGROUND,
        LifecycleTransition::EnteredBackground => LIFECYCLE_PHASE_BACKGROUND,
    };
    if let Some((snapshot, _invocation)) = observer.begin_invocation() {
        match snapshot {
            LifecycleHandleSnapshot::CFfi(registration) => {
                // UB guard: the foreign callback may panic / raise; an unwind
                // across the C ABI boundary is undefined behaviour.
                let _ = crate::ffi_guard::guard_ffi_callback("lifecycle observer", || {
                    (registration.callback)(
                        registration.context as *mut std::ffi::c_void,
                        phase_code,
                    );
                });
            }
            LifecycleHandleSnapshot::Native(observer) => {
                // Panic containment: a Swift/Kotlin throw must not unwind into
                // the actor thread (D6). The phase code is a copied `u32` — no
                // Rust lock is held across the call.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    observer(phase_code);
                }));
            }
        }
    }
    Some(transition)
}

#[cfg(test)]
#[path = "lifecycle_quiescence_tests.rs"]
mod lifecycle_quiescence_tests;

#[cfg(test)]
mod tests {
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
        slot.set_registration(Some(LifecycleObserverRegistration {
            context: 0,
            callback: observer_shim,
        }));
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
}
