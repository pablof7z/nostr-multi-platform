//! Drain-gate for the action-result observer (M14-C-tail / #2429).
//!
//! Replaces the prior `Arc<Mutex<Option<Box<dyn Fn>>>>` slot — whose
//! `deliver_result` held the mutex ACROSS the observer call (mutual-exclusion
//! quiescence) — with the same `in_flight` + [`Condvar`] drain used by
//! [`crate::capability_socket::CapabilityCallbackGate`] and the lifecycle gate.
//!
//! Why the change: holding the lock across the foreign call (a) risks a
//! re-entrancy deadlock if a host observer calls `set_result_observer` from
//! inside `on_action_result`, and (b) gives no `clear_result_observer` with a
//! drain-before-return guarantee — needed so M14-D can delete the C-ABI and so
//! the UniFFI `ActionResultObserver` ARC can be released safely on teardown.
//!
//! Contract: after [`Self::set_observer`] / [`Self::clear`] returns, the
//! previous observer is neither registered nor mid-invocation, so its captured
//! state (a UniFFI callback ARC) may be dropped immediately.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use crate::substrate::ActionResult;

/// Host-registered action-result observer. `Arc` (not `Box`) so it can be
/// cloned out from under the gate lock and invoked without holding it.
pub(crate) type ResultObserver = Arc<dyn Fn(ActionResult) + Send + Sync + 'static>;

struct ResultObserverGateInner {
    observer: Option<ResultObserver>,
    in_flight: u32,
}

/// Quiescence-safe slot for the action-result observer.
pub(crate) struct ResultObserverGate {
    inner: Mutex<ResultObserverGateInner>,
    drained: Condvar,
}

impl ResultObserverGate {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(ResultObserverGateInner {
                observer: None,
                in_flight: 0,
            }),
            drained: Condvar::new(),
        }
    }

    /// Replace or clear the observer, then wait for any in-flight delivery to
    /// complete before returning (drain-before-return).
    ///
    /// Re-entrancy: an observer MUST NOT call this from inside
    /// `on_action_result` — the setter waits for that very delivery to finish.
    pub(crate) fn set_observer(&self, observer: Option<ResultObserver>) {
        let mut guard = self.lock_inner();
        guard.observer = observer;
        drop(self.wait_drained(guard));
    }

    /// Clear the observer and drain in-flight deliveries. Idempotent.
    pub(crate) fn clear(&self) {
        self.set_observer(None);
    }

    pub(crate) fn is_registered(&self) -> bool {
        self.lock_inner().observer.is_some()
    }

    /// Deliver `result` to the registered observer, if any.
    ///
    /// Snapshots the observer + increments `in_flight` under the lock, then
    /// releases the lock BEFORE invoking — so a concurrent `set_observer` /
    /// `clear` blocks (drains) rather than racing, and a re-entrant observer
    /// cannot deadlock against a held lock. The [`InvocationGuard`] decrements
    /// `in_flight` and notifies waiters even if the observer panics.
    ///
    /// D6: the observer is untrusted host plugin code on the FFI dispatch
    /// thread; a panic is caught (the result is dropped, the observer stays
    /// registered so the next delivery still fires).
    pub(crate) fn deliver(&self, result: ActionResult) {
        let observer = {
            let mut guard = self.lock_inner();
            let Some(observer) = guard.observer.as_ref().map(Arc::clone) else {
                return;
            };
            guard.in_flight = guard.in_flight.saturating_add(1);
            observer
        };
        let _invocation = InvocationGuard { gate: self };
        let _ = catch_unwind(AssertUnwindSafe(move || observer(result)));
    }

    fn lock_inner(&self) -> MutexGuard<'_, ResultObserverGateInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn finish_invocation(&self) {
        let mut guard = self.lock_inner();
        guard.in_flight = guard.in_flight.saturating_sub(1);
        if guard.in_flight == 0 {
            self.drained.notify_all();
        }
    }

    fn wait_drained<'a>(
        &'a self,
        guard: MutexGuard<'a, ResultObserverGateInner>,
    ) -> MutexGuard<'a, ResultObserverGateInner> {
        self.drained
            .wait_while(guard, |inner| inner.in_flight > 0)
            .unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for ResultObserverGate {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard: decrements `in_flight` and notifies waiters on drop, so a
/// panicking observer still drains the gate.
struct InvocationGuard<'a> {
    gate: &'a ResultObserverGate,
}

impl Drop for InvocationGuard<'_> {
    fn drop(&mut self) {
        self.gate.finish_invocation();
    }
}
