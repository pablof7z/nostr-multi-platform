//! Cancelable one-shot timer for the browser relay maintenance schedule (#2069).
//!
//! # Design
//!
//! `CancelableTimer` wraps a `gloo_timers::callback::Timeout` on wasm32 and a
//! pure state-tracking struct on native (for unit tests). In both cases
//! `arm(delay_ms, cb)` schedules a callback only if the requested delay is
//! sooner than any currently-armed deadline (replace-only-if-sooner). This
//! prevents needless re-scheduling when pump() is called more often than the
//! maintenance deadline.
//!
//! # Worker compatibility (#2069)
//!
//! `gloo_timers::callback::Timeout` schedules via `js_sys::global()` rather
//! than `web_sys::window()`. `js_sys::global()` resolves to whichever global
//! scope is active — both `Window` and `WorkerGlobalScope` — so the timer
//! fires correctly when the kernel runs inside a Worker (the primary
//! nmp-browser-runtime deployment context).
//!
//! # Native test support
//!
//! On `#[cfg(not(target_arch = "wasm32"))]` builds the struct has no real timer
//! and no OS-level scheduling. `armed_delay_ms_for_test()` allows unit tests to
//! assert that the timer was armed with the expected delay.

use std::cell::Cell;
use std::rc::Rc;

/// Inner state shared between the `CancelableTimer` and its wasm32 callback
/// closure so the callback can clear the `armed_delay_ms` when it fires.
struct Inner {
    armed_delay_ms: Cell<Option<u32>>,
}

/// One-shot cancelable timer — arms-if-sooner semantics (#2069).
pub(crate) struct CancelableTimer {
    inner: Rc<Inner>,
    /// The live Timeout on wasm32. `None` on native or when canceled.
    #[cfg(target_arch = "wasm32")]
    timeout: Option<gloo_timers::callback::Timeout>,
}

impl CancelableTimer {
    /// Construct an unarmed timer.
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                armed_delay_ms: Cell::new(None),
            }),
            #[cfg(target_arch = "wasm32")]
            timeout: None,
        }
    }

    /// Arm the timer only if `delay_ms` is sooner than the currently armed
    /// deadline (or if no timer is currently armed). Returns `true` when the
    /// timer was (re)armed.
    ///
    /// On wasm32 the callback fires after `delay_ms` milliseconds and then
    /// calls `cb`. On native the callback is not scheduled; tests drive the
    /// timer manually.
    pub(crate) fn arm(&mut self, delay_ms: u32, _cb: Rc<dyn Fn()>) -> bool {
        // If a previously-fired timer left stale state (the callback ran, set
        // armed_delay_ms back to None), `Cell::get()` returns None and we arm
        // unconditionally. This is the normal renewal path.
        if let Some(cur) = self.inner.armed_delay_ms.get() {
            if delay_ms >= cur {
                return false; // Current deadline is sooner or equal — keep it.
            }
            // Canceling the current timer before arming a sooner one.
            self.cancel();
        }

        self.inner.armed_delay_ms.set(Some(delay_ms));

        #[cfg(target_arch = "wasm32")]
        {
            let inner = Rc::clone(&self.inner);
            self.timeout = Some(gloo_timers::callback::Timeout::new(delay_ms, move || {
                // Clear armed state BEFORE calling the user callback so that
                // the callback (which typically calls pump() → arm()) sees the
                // timer as unarmed and can re-arm for the next deadline.
                inner.armed_delay_ms.set(None);
                (_cb)();
            }));
        }

        true
    }

    /// Cancel the currently armed timer (no-op if not armed).
    pub(crate) fn cancel(&mut self) {
        self.inner.armed_delay_ms.set(None);
        #[cfg(target_arch = "wasm32")]
        {
            self.timeout = None; // Dropping the Timeout calls clearTimeout.
        }
    }

    /// Whether the timer is currently armed (native tests only).
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)] // used in unit tests; dead_code lint does not count test usage
    pub(crate) fn armed_delay_ms_for_test(&self) -> Option<u32> {
        self.inner.armed_delay_ms.get()
    }
}

impl Default for CancelableTimer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_wake() -> Rc<dyn Fn()> {
        Rc::new(|| {})
    }

    #[test]
    fn unarmed_by_default() {
        let t = CancelableTimer::new();
        assert_eq!(t.armed_delay_ms_for_test(), None);
    }

    #[test]
    fn arm_sets_delay() {
        let mut t = CancelableTimer::new();
        t.arm(500, noop_wake());
        assert_eq!(t.armed_delay_ms_for_test(), Some(500));
    }

    #[test]
    fn arm_replaces_if_sooner() {
        let mut t = CancelableTimer::new();
        t.arm(1000, noop_wake());
        assert_eq!(t.armed_delay_ms_for_test(), Some(1000));

        let replaced = t.arm(200, noop_wake());
        assert!(replaced, "sooner delay must replace");
        assert_eq!(t.armed_delay_ms_for_test(), Some(200));
    }

    #[test]
    fn arm_does_not_replace_if_not_sooner() {
        let mut t = CancelableTimer::new();
        t.arm(200, noop_wake());
        let replaced = t.arm(500, noop_wake());
        assert!(!replaced, "later delay must not replace");
        assert_eq!(t.armed_delay_ms_for_test(), Some(200));
    }

    #[test]
    fn arm_does_not_replace_if_equal() {
        let mut t = CancelableTimer::new();
        t.arm(100, noop_wake());
        let replaced = t.arm(100, noop_wake());
        assert!(!replaced, "equal delay must not replace");
    }

    #[test]
    fn cancel_clears_armed_state() {
        let mut t = CancelableTimer::new();
        t.arm(300, noop_wake());
        t.cancel();
        assert_eq!(t.armed_delay_ms_for_test(), None);
    }

    #[test]
    fn arm_after_cancel_works() {
        let mut t = CancelableTimer::new();
        t.arm(300, noop_wake());
        t.cancel();
        let armed = t.arm(50, noop_wake());
        assert!(armed);
        assert_eq!(t.armed_delay_ms_for_test(), Some(50));
    }
}
