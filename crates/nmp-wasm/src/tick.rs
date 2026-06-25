//! Event/deadline-driven runtime drain for the wasm32 runtime.
//!
//! The runtime has a small amount of kernel-owned maintenance work that is not
//! tied to a single host request: subscription lifecycle drains, claim
//! expansion, publish retries/timeouts, and composition-root post-event drains.
//! The old production path drove that work from a fixed browser interval. This
//! module keeps the same deterministic reducer drain, but production schedules
//! it only from explicit events or a previously-armed runtime deadline.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use nmp_core::{KernelReducer, OutboundMessage, SignRoundTripCompletion};

use crate::protocol::{CapabilityFailure, WorkerEvent};
use crate::runtime::PendingSignedPublish;

/// Coarse post-event maintenance deadline.
///
/// This is not a recurring interval. It is the delay for one event-triggered
/// browser deadline; follow-up wakes are armed only from explicit
/// kernel-declared deadlines.
pub(crate) const RUNTIME_EVENT_DRAIN_MS: u32 = 1_000;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum WakePolicy {
    /// Fire once after an event that may have queued immediate maintenance.
    Event,
    /// Fire at the next deadline the kernel/reducer reported.
    KernelDeadline { delay_ms: u32 },
}

impl WakePolicy {
    #[must_use]
    fn delay_ms(self) -> u32 {
        match self {
            Self::Event => RUNTIME_EVENT_DRAIN_MS,
            Self::KernelDeadline { delay_ms } => delay_ms,
        }
    }
}

#[must_use]
pub(crate) fn event_or_kernel_policy(reducer: &Rc<RefCell<KernelReducer>>) -> WakePolicy {
    let now = nmp_core::time::Instant::now();
    next_runtime_deadline_delay_ms(reducer, now)
        .filter(|delay| *delay < RUNTIME_EVENT_DRAIN_MS)
        .map_or(WakePolicy::Event, |delay_ms| WakePolicy::KernelDeadline {
            delay_ms,
        })
}

#[must_use]
pub(crate) fn next_runtime_deadline_delay_ms(
    reducer: &Rc<RefCell<KernelReducer>>,
    now: nmp_core::time::Instant,
) -> Option<u32> {
    let reducer = reducer.borrow();
    reducer
        .next_runtime_deadline_delay_ms()
        .into_iter()
        .chain(reducer.next_sign_roundtrip_deadline_delay_ms(now))
        .min()
}

#[derive(Default)]
pub(crate) struct RuntimeDeadline {
    armed: bool,
    armed_delay_ms: Option<u32>,
    requested: u64,
    fired: u64,
    #[cfg(target_arch = "wasm32")]
    timeout: Option<gloo_timers::callback::Timeout>,
}

impl RuntimeDeadline {
    fn request(&mut self, policy: WakePolicy) -> bool {
        let delay_ms = policy.delay_ms();
        if self.armed {
            let should_replace = match self.armed_delay_ms {
                Some(current) => delay_ms < current,
                None => true,
            };
            if !should_replace {
                return false;
            }
            #[cfg(target_arch = "wasm32")]
            {
                self.timeout = None;
            }
        } else {
            self.armed = true;
        }
        self.armed_delay_ms = Some(delay_ms);
        self.requested = self.requested.saturating_add(1);
        true
    }

    fn begin_fire(&mut self) {
        self.armed = false;
        self.armed_delay_ms = None;
        self.fired = self.fired.saturating_add(1);
        #[cfg(target_arch = "wasm32")]
        {
            self.timeout = None;
        }
    }

    fn finish_fire(&mut self, outcome: &DrainOutcome) -> Option<WakePolicy> {
        outcome
            .next_deadline_delay_ms
            .map(|delay_ms| WakePolicy::KernelDeadline { delay_ms })
    }

    fn cancel(&mut self) {
        self.armed = false;
        self.armed_delay_ms = None;
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
    pub(crate) fn armed_delay_ms_for_test(&self) -> Option<u32> {
        self.armed_delay_ms
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
    pub(crate) next_deadline_delay_ms: Option<u32>,
    pub(crate) worker_events: Vec<WorkerEvent>,
}

/// Drive one deterministic reducer maintenance pass.
///
/// The reducer borrow is released before the post-event drain hook runs. If the
/// hook exists, run one additional reducer pass afterwards so work the hook queued
/// is serviced by the same explicit wake instead of depending on another
/// timer event.
pub(crate) fn drain_once(
    reducer: &Rc<RefCell<KernelReducer>>,
    pending_signed_publishes: &Rc<RefCell<HashMap<String, PendingSignedPublish>>>,
    post_event_drain: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> DrainOutcome {
    let now = nmp_core::time::Instant::now();
    let (mut outbound, completions) = {
        let mut reducer = reducer.borrow_mut();
        let outbound = reducer.tick_at(now);
        let completions = reducer.drive_sign_roundtrip_timeouts_at(now);
        (outbound, completions)
    };
    if run_post_event_drain(post_event_drain) {
        let mut reducer_ref = reducer.borrow_mut();
        outbound.extend(reducer_ref.tick_at(now));
        let mut post_drain_completions = reducer_ref.drive_sign_roundtrip_timeouts_at(now);
        drop(reducer_ref);
        let mut completions = completions;
        completions.append(&mut post_drain_completions);
        let reducer_ref = reducer.borrow();
        let dirty = reducer_ref.changed_since_emit();
        drop(reducer_ref);
        let next_deadline_delay_ms = next_runtime_deadline_delay_ms(reducer, now);
        let worker_events = signer_completion_events(completions, pending_signed_publishes);
        return DrainOutcome {
            outbound,
            dirty,
            next_deadline_delay_ms,
            worker_events,
        };
    }
    let reducer_ref = reducer.borrow();
    let dirty = reducer_ref.changed_since_emit();
    drop(reducer_ref);
    let next_deadline_delay_ms = next_runtime_deadline_delay_ms(reducer, now);
    let worker_events = signer_completion_events(completions, pending_signed_publishes);
    DrainOutcome {
        outbound,
        dirty,
        next_deadline_delay_ms,
        worker_events,
    }
}

pub(crate) fn signer_completion_events(
    completions: Vec<SignRoundTripCompletion>,
    pending_signed_publishes: &Rc<RefCell<HashMap<String, PendingSignedPublish>>>,
) -> Vec<WorkerEvent> {
    let mut events = Vec::new();
    for completion in completions {
        match completion.outcome {
            Ok(signed_json) => events.push(WorkerEvent::SignCompleted {
                correlation_id: completion.correlation_id,
                signed_json,
            }),
            Err(reason) => {
                events.push(WorkerEvent::SignFailed {
                    correlation_id: completion.correlation_id.clone(),
                    reason: reason.clone(),
                });
                if let Some(pending) = pending_signed_publishes
                    .borrow_mut()
                    .remove(&completion.correlation_id)
                {
                    events.push(WorkerEvent::CapabilityFailure(CapabilityFailure {
                        capability: pending.action_namespace,
                        correlation_id: pending.action_correlation_id,
                        reason,
                    }));
                }
            }
        }
    }
    events
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
    pending_signed_publishes: &Rc<RefCell<HashMap<String, PendingSignedPublish>>>,
    post_event_drain: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> Option<DrainOutcome> {
    if !deadline.borrow().armed {
        return None;
    }
    deadline.borrow_mut().begin_fire();
    let outcome = drain_once(reducer, pending_signed_publishes, post_event_drain);
    let next_policy = deadline.borrow_mut().finish_fire(&outcome);
    if let Some(policy) = next_policy {
        request_deadline_for_test(deadline, policy);
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
    pending_signed_publishes: Rc<RefCell<HashMap<String, PendingSignedPublish>>>,
    event_callback: Rc<RefCell<Option<js_sys::Function>>>,
    post_event_drain: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) {
    let delay_ms = policy.delay_ms();
    if !deadline.borrow_mut().request(policy) {
        return;
    }

    let callback_deadline = Rc::clone(&deadline);
    let callback_reducer = Rc::clone(&reducer);
    let callback_drivers = Rc::clone(&drivers);
    let callback_handlers = Rc::clone(&handlers_slot);
    let callback_snapshot = Rc::clone(&snapshot_callback);
    let callback_meta = Rc::clone(&meta);
    let callback_pending_signed_publishes = Rc::clone(&pending_signed_publishes);
    let callback_event = Rc::clone(&event_callback);
    let callback_post_drain = Rc::clone(&post_event_drain);

    let timeout = gloo_timers::callback::Timeout::new(delay_ms, move || {
        callback_deadline.borrow_mut().begin_fire();
        let outcome = drain_once(
            &callback_reducer,
            &callback_pending_signed_publishes,
            &callback_post_drain,
        );
        crate::relay_pool::fan_out_outbound(
            &callback_drivers,
            &callback_handlers,
            &outcome.outbound,
        );
        push_worker_events_if_callback(&callback_event, &outcome.worker_events);
        if outcome.dirty {
            crate::snapshot::push_snapshot_if_callback(
                &callback_snapshot,
                &callback_reducer,
                &callback_meta,
            );
        }
        let next_policy = callback_deadline.borrow_mut().finish_fire(&outcome);
        if let Some(policy) = next_policy {
            request_runtime_deadline(
                Rc::clone(&callback_deadline),
                policy,
                Rc::clone(&callback_reducer),
                Rc::clone(&callback_drivers),
                Rc::clone(&callback_handlers),
                Rc::clone(&callback_snapshot),
                Rc::clone(&callback_meta),
                Rc::clone(&callback_pending_signed_publishes),
                Rc::clone(&callback_event),
                Rc::clone(&callback_post_drain),
            );
        }
    });
    deadline.borrow_mut().timeout = Some(timeout);
}

#[cfg(target_arch = "wasm32")]
fn push_worker_events_if_callback(
    callback: &Rc<RefCell<Option<js_sys::Function>>>,
    events: &[WorkerEvent],
) {
    let callback_ref = callback.borrow();
    let Some(callback_fn) = callback_ref.as_ref() else {
        return;
    };
    for event in events {
        let Ok(json) = serde_json::to_string(event) else {
            continue;
        };
        let _ = callback_fn.call1(
            &wasm_bindgen::JsValue::NULL,
            &wasm_bindgen::JsValue::from_str(&json),
        );
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn cancel_deadline(deadline: &Rc<RefCell<RuntimeDeadline>>) {
    deadline.borrow_mut().cancel();
}
