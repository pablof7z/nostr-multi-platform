//! Event/deadline-driven runtime drain for the wasm32 runtime.
//!
//! The runtime has a small amount of kernel-owned maintenance work that is not
//! tied to a single host request: subscription lifecycle drains, claim
//! expansion, publish retries/timeouts, and composition-root post-event drains.
//! The old production path drove that work from a fixed browser interval. This
//! module keeps the same deterministic reducer drain, but production schedules
//! it only from explicit events or a previously-armed runtime deadline.

use std::cell::RefCell;
use std::rc::Rc;

use nmp_core::{KernelReducer, OutboundMessage};

/// Coarse runtime-deadline cadence.
///
/// This is not a recurring interval. It is the delay for one armed browser
/// deadline; the callback decides whether deadline-bearing work remains before
/// arming another one.
pub(crate) const RUNTIME_DEADLINE_MS: u32 = 1_000;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum WakePolicy {
    /// Fire once after an event that may have queued immediate maintenance.
    Single,
    /// Continue deadline checks until the tracked work resolves.
    Tracked,
}

#[derive(Default)]
pub(crate) struct RuntimeDeadline {
    armed: bool,
    tracked: bool,
    requested: u64,
    fired: u64,
    #[cfg(target_arch = "wasm32")]
    timeout: Option<gloo_timers::callback::Timeout>,
}

impl RuntimeDeadline {
    fn request(&mut self, policy: WakePolicy) -> bool {
        if matches!(policy, WakePolicy::Tracked) {
            self.tracked = true;
        }
        if self.armed {
            return false;
        }
        self.armed = true;
        self.requested = self.requested.saturating_add(1);
        true
    }

    fn begin_fire(&mut self) {
        self.armed = false;
        self.fired = self.fired.saturating_add(1);
        #[cfg(target_arch = "wasm32")]
        {
            self.timeout = None;
        }
    }

    fn finish_fire(&mut self, outcome: &DrainOutcome) -> bool {
        if outcome.has_outbound() {
            self.tracked = true;
        } else if outcome.dirty {
            self.tracked = false;
        }
        self.tracked
    }

    fn cancel(&mut self) {
        self.armed = false;
        self.tracked = false;
        #[cfg(target_arch = "wasm32")]
        {
            self.timeout = None;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn armed_for_test(&self) -> bool {
        self.armed
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn requested_for_test(&self) -> u64 {
        self.requested
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn fired_for_test(&self) -> u64 {
        self.fired
    }
}

pub(crate) struct DrainOutcome {
    pub(crate) outbound: Vec<OutboundMessage>,
    pub(crate) dirty: bool,
}

impl DrainOutcome {
    #[must_use]
    pub(crate) fn has_outbound(&self) -> bool {
        !self.outbound.is_empty()
    }
}

/// Drive one deterministic reducer maintenance pass.
///
/// The reducer borrow is released before the post-event drain hook runs. If the
/// hook exists, run one additional reducer pass afterwards so work the hook queued
/// is serviced by the same explicit wake instead of depending on another
/// periodic timer.
pub(crate) fn drain_once(
    reducer: &Rc<RefCell<KernelReducer>>,
    post_event_drain: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> DrainOutcome {
    let mut outbound = reducer.borrow_mut().tick();
    if run_post_event_drain(post_event_drain) {
        outbound.extend(reducer.borrow_mut().tick());
    }
    let dirty = reducer.borrow().changed_since_emit();
    DrainOutcome { outbound, dirty }
}

fn run_post_event_drain(post_event_drain: &Rc<RefCell<Option<Rc<dyn Fn()>>>>) -> bool {
    let drain = match post_event_drain.try_borrow() {
        Ok(slot) => slot.as_ref().map(Rc::clone),
        Err(_) => None,
    };
    if let Some(drain) = drain {
        drain();
        true
    } else {
        false
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn request_deadline_for_test(
    deadline: &Rc<RefCell<RuntimeDeadline>>,
    policy: WakePolicy,
) {
    let _ = deadline.borrow_mut().request(policy);
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn cancel_deadline(deadline: &Rc<RefCell<RuntimeDeadline>>) {
    deadline.borrow_mut().cancel();
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn fire_deadline_for_test(
    deadline: &Rc<RefCell<RuntimeDeadline>>,
    reducer: &Rc<RefCell<KernelReducer>>,
    post_event_drain: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> Option<DrainOutcome> {
    if !deadline.borrow().armed {
        return None;
    }
    deadline.borrow_mut().begin_fire();
    let outcome = drain_once(reducer, post_event_drain);
    if deadline.borrow_mut().finish_fire(&outcome) {
        request_deadline_for_test(deadline, WakePolicy::Single);
    }
    Some(outcome)
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn request_runtime_deadline(
    deadline: Rc<RefCell<RuntimeDeadline>>,
    policy: WakePolicy,
    reducer: Rc<RefCell<KernelReducer>>,
    drivers: Rc<RefCell<Vec<Rc<nmp_network::browser_driver::BrowserRelayDriver>>>>,
    handlers_slot: Rc<RefCell<Option<nmp_network::browser_driver::BrowserKernelHandlers>>>,
    snapshot_callback: Rc<RefCell<Option<js_sys::Function>>>,
    meta: Rc<RefCell<crate::snapshot::RuntimeMeta>>,
    post_event_drain: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) {
    if !deadline.borrow_mut().request(policy) {
        return;
    }

    let callback_deadline = Rc::clone(&deadline);
    let callback_reducer = Rc::clone(&reducer);
    let callback_drivers = Rc::clone(&drivers);
    let callback_handlers = Rc::clone(&handlers_slot);
    let callback_snapshot = Rc::clone(&snapshot_callback);
    let callback_meta = Rc::clone(&meta);
    let callback_post_drain = Rc::clone(&post_event_drain);

    let timeout = gloo_timers::callback::Timeout::new(RUNTIME_DEADLINE_MS, move || {
        callback_deadline.borrow_mut().begin_fire();
        let outcome = drain_once(&callback_reducer, &callback_post_drain);
        crate::relay_pool::fan_out_outbound(
            &callback_drivers,
            &callback_handlers,
            &outcome.outbound,
        );
        if outcome.dirty {
            crate::snapshot::push_snapshot_if_callback(
                &callback_snapshot,
                &callback_reducer,
                &callback_meta,
            );
        }
        let should_rearm = callback_deadline.borrow_mut().finish_fire(&outcome);
        if should_rearm {
            request_runtime_deadline(
                Rc::clone(&callback_deadline),
                WakePolicy::Single,
                Rc::clone(&callback_reducer),
                Rc::clone(&callback_drivers),
                Rc::clone(&callback_handlers),
                Rc::clone(&callback_snapshot),
                Rc::clone(&callback_meta),
                Rc::clone(&callback_post_drain),
            );
        }
    });
    deadline.borrow_mut().timeout = Some(timeout);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn cancel_deadline(deadline: &Rc<RefCell<RuntimeDeadline>>) {
    deadline.borrow_mut().cancel();
}
