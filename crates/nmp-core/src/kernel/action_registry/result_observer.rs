use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use crate::substrate::ActionResult;

type ResultObserver = Arc<dyn Fn(ActionResult) + Send + Sync + 'static>;

struct ResultObserverGateInner {
    observer: Option<ResultObserver>,
    in_flight: u32,
}

/// Quiescence-safe slot for the action-result observer.
///
/// Set/replace/clear waits for every callback that was already copied out of
/// the slot to return. After a setter returns, the previous observer is neither
/// registered nor mid-invocation.
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

    pub(crate) fn set_observer(&self, observer: ResultObserver) {
        let mut guard = self.lock_inner();
        guard.observer = Some(observer);
        drop(self.wait_drained(guard));
    }

    pub(crate) fn clear_observer(&self) {
        let mut guard = self.lock_inner();
        guard.observer = None;
        drop(self.wait_drained(guard));
    }

    pub(crate) fn deliver(&self, result: ActionResult) {
        let Some((observer, _invocation)) = self.begin_invocation() else {
            return;
        };
        let _ = catch_unwind(AssertUnwindSafe(|| observer(result)));
    }

    fn begin_invocation(&self) -> Option<(ResultObserver, ResultObserverInvocation<'_>)> {
        let mut guard = self.lock_inner();
        let observer = Arc::clone(guard.observer.as_ref()?);
        guard.in_flight = guard.in_flight.saturating_add(1);
        Some((observer, ResultObserverInvocation { gate: self }))
    }

    fn finish_invocation(&self) {
        let mut guard = self.lock_inner();
        guard.in_flight = guard.in_flight.saturating_sub(1);
        if guard.in_flight == 0 {
            self.drained.notify_all();
        }
    }

    fn lock_inner(&self) -> MutexGuard<'_, ResultObserverGateInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
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

struct ResultObserverInvocation<'a> {
    gate: &'a ResultObserverGate,
}

impl Drop for ResultObserverInvocation<'_> {
    fn drop(&mut self) {
        self.gate.finish_invocation();
    }
}

pub(crate) type ResultObserverSlot = Arc<ResultObserverGate>;

pub(crate) fn new_result_observer_slot() -> ResultObserverSlot {
    Arc::new(ResultObserverGate::new())
}
