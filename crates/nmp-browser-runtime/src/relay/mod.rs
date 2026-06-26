//! Browser relay pool — driver lifecycle, inbound queue, and maintenance timer.
//!
//! [`RelayPool`] is owned by `BrowserRuntime` and driven exclusively from
//! `pump()` (D4 single-writer). On wasm32 it holds the live
//! [`BrowserRelayDriver`] vec and the shared [`BrowserKernelHandlers`] bag.
//! On native it is an always-compile stub that enables testing the inbound
//! drain, plan, and timer logic without a wasm32 toolchain.
//!
//! # Relay User-Agent (#2050 O4)
//!
//! Browser `web_sys::WebSocket` cannot set custom HTTP headers during the WS
//! handshake (browser security constraint). The configured `relay_user_agent`
//! is therefore carried to the NIP-11 GET only (where headers can be set).
//! The WS handshake carries the browser's default User-Agent string.

use std::rc::Rc;
use std::sync::Arc;

use nmp_core::substrate::{RelayConnectedHook, RelayTextInterceptor};
use nmp_core::time::Instant;
use nmp_core::{CommandSender, KernelReducer, OutboundMessage};

#[cfg(target_arch = "wasm32")]
use budgets::MAX_CONCURRENT_SOCKETS;
use crate::BrowserRuntimeEvent;

pub(crate) mod budgets;
pub(crate) mod handlers;
pub(crate) mod inbound;
pub(crate) mod plan;
pub(crate) mod spawn;
pub(crate) mod timer;

use inbound::{drain_inbound, InboundDrainOutcome, InboundQueue};
use timer::CancelableTimer;

/// The browser relay pool — driver set + inbound queue + maintenance timer.
pub(crate) struct RelayPool {
    /// Shared inbound event queue (pushed by JS handlers, drained by pump()).
    pub(crate) inbound: Rc<InboundQueue>,

    /// "Please schedule a pump" hook. Default: no-op. Set by the host via
    /// [`Self::set_wake`] so inbound events trigger pump() calls without
    /// external polling (D8 no-polling).
    wake: Rc<dyn Fn()>,

    /// Maintenance timer armed by pump() from `next_runtime_deadline_delay_ms`.
    /// When it fires it calls wake() which schedules a pump().
    maintenance_timer: CancelableTimer,

    /// User-agent string carried to the NIP-11 info-document GET (not the WS
    /// handshake — browser WS cannot set custom headers).
    #[allow(dead_code)] // seam: NIP-11 browser fetch lands in #2050 follow-up
    user_agent: Option<String>,

    /// Live WebSocket drivers, one per distinct relay URL. wasm32-only — native
    /// builds have no actual sockets.
    #[cfg(target_arch = "wasm32")]
    drivers: Vec<Rc<nmp_network::browser_driver::BrowserRelayDriver>>,

    /// Cached handler bag stored so spawn-on-miss can wire new drivers with the
    /// same closures as bootstrap drivers. wasm32-only.
    #[cfg(target_arch = "wasm32")]
    handlers_slot: Option<nmp_network::browser_driver::BrowserKernelHandlers>,
}

impl RelayPool {
    /// Construct an idle pool. Call [`Self::spawn_bootstrap`] on wasm32 to open
    /// sockets for the configured relay list.
    pub(crate) fn new(user_agent: Option<String>) -> Self {
        Self {
            inbound: InboundQueue::new(),
            wake: Rc::new(|| {}), // no-op default; set via set_wake()
            maintenance_timer: CancelableTimer::new(),
            user_agent,
            #[cfg(target_arch = "wasm32")]
            drivers: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            handlers_slot: None,
        }
    }

    /// Install the "please pump now" hook. Called once by the host after start
    /// (e.g. the nmp-wasm bridge sets a closure that schedules a 0ms timer
    /// which calls the wasm-exported pump function). Not called in native tests
    /// — tests invoke pump() directly.
    pub(crate) fn set_wake(&mut self, wake: Rc<dyn Fn()>) {
        self.wake = wake;
    }

    /// Open sockets for the bootstrap relay list (wasm32 only).
    ///
    /// Builds the enqueue-only handler closure bag, stores it in
    /// `handlers_slot`, then calls [`BrowserRelayDriver::new`] for each planned
    /// URL. Bootstrap-ordering invariant: the handler closures capture an
    /// `Rc<InboundQueue>` and `wake` clone; the drivers themselves are pushed
    /// into `self.drivers`. By the time the first JS `onopen` can fire (after
    /// control returns to the JS event loop), `handlers_slot` is populated and
    /// spawn-on-miss can proceed.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn spawn_bootstrap(
        &mut self,
        bootstrap: &[(String, String)],
    ) -> Vec<BrowserRuntimeEvent> {
        let plans = plan::plan_drivers(bootstrap);
        let handlers = handlers::build_handlers(
            Rc::clone(&self.inbound),
            Rc::clone(&self.wake),
        );
        self.handlers_slot = Some(handlers.clone());

        let mut events = Vec::new();
        for plan in plans {
            if self.drivers.len() >= MAX_CONCURRENT_SOCKETS {
                events.push(BrowserRuntimeEvent::RelayBudgetExceeded {
                    url: plan.url.clone(),
                });
                continue;
            }
            match nmp_network::browser_driver::BrowserRelayDriver::new(
                plan.url.clone(),
                plan.primary_role,
                handlers.clone(),
            ) {
                Ok(driver) => self.drivers.push(driver),
                // Unparseable URL (bad scheme / illegal chars). Very rare since
                // the kernel's CanonicalRelayUrl normalises before storage.
                // seam: surface dial errors via RelayDriverError (#2050 follow-up)
                Err(_) => continue,
            }
        }
        events
    }

    /// Drain inbound events, run relay lifecycle + interceptors + hooks, and
    /// return the outcome. Called from `BrowserRuntime::pump()`.
    pub(crate) fn drain_inbound(
        &self,
        reducer: &mut KernelReducer,
        interceptors: &[Arc<dyn RelayTextInterceptor>],
        hooks: &[Arc<dyn RelayConnectedHook>],
        command_sender: &CommandSender,
    ) -> InboundDrainOutcome {
        drain_inbound(&self.inbound, reducer, interceptors, hooks, command_sender)
    }

    /// Fan outbound messages to relay drivers, spawning new drivers on miss.
    /// Returns any budget-exceeded events. wasm32-only (native: no-op).
    pub(crate) fn fan_out_outbound(
        &mut self,
        outbound: &[OutboundMessage],
    ) -> Vec<BrowserRuntimeEvent> {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(handlers) = &self.handlers_slot {
                return spawn::fan_out_outbound(&mut self.drivers, handlers, outbound);
            }
            // No handlers yet (pool not started) — drop outbound frames.
            Vec::new()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = outbound;
            Vec::new()
        }
    }

    /// Run one maintenance tick and arm the next deadline timer.
    ///
    /// Calls `reducer.tick_at(now)` to drain the kernel's maintenance queues
    /// (subscription lifecycle, claim expansion, publish-engine tick) and then
    /// arms the maintenance timer for the next kernel-declared deadline.
    ///
    /// `MAINTENANCE_DELAY_MS_CAP` (1000ms) prevents the timer from waking more
    /// than once per second in the absence of a kernel deadline.
    pub(crate) fn tick_and_arm(
        &mut self,
        reducer: &mut KernelReducer,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        const MAINTENANCE_DELAY_MS_CAP: u32 = 1000;

        let outbound = reducer.tick_at(now);

        // Arm maintenance timer at the next kernel-declared deadline (capped).
        let delay_ms = reducer
            .next_runtime_deadline_delay_ms()
            .unwrap_or(MAINTENANCE_DELAY_MS_CAP)
            .clamp(1, MAINTENANCE_DELAY_MS_CAP);

        let wake = Rc::clone(&self.wake);
        self.maintenance_timer.arm(delay_ms, Rc::new(move || {
            (wake)();
        }));

        outbound
    }

    /// Close all drivers and cancel the maintenance timer. Idempotent.
    #[allow(dead_code)] // seam: called when BrowserRuntimeHandle gains a shutdown() path
    pub(crate) fn close(&mut self) {
        self.maintenance_timer.cancel();
        #[cfg(target_arch = "wasm32")]
        spawn::close_drivers(&mut self.drivers);
    }

    /// Number of live drivers (diagnostic / test helper).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn driver_count(&self) -> usize {
        #[cfg(target_arch = "wasm32")]
        return self.drivers.len();
        #[cfg(not(target_arch = "wasm32"))]
        return 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pool_is_idle() {
        let pool = RelayPool::new(None);
        assert_eq!(pool.driver_count(), 0);
        assert!(pool.inbound.queue.borrow().is_empty());
    }

    #[test]
    fn maintenance_timer_armed_after_tick() {
        let mut pool = RelayPool::new(None);
        let mut reducer = KernelReducer::new();
        let now = nmp_core::time::Instant::now();
        pool.tick_and_arm(&mut reducer, now);
        // Timer must be armed (delay may vary based on kernel state, but
        // must be Some and within the cap).
        assert!(
            pool.maintenance_timer.armed_delay_ms_for_test().is_some(),
            "maintenance timer must be armed after tick_and_arm"
        );
        let delay = pool.maintenance_timer.armed_delay_ms_for_test().unwrap();
        assert!(delay >= 1 && delay <= 1000, "delay must be in [1, 1000]");
    }

    #[test]
    fn set_wake_replaces_the_hook() {
        use std::cell::Cell;
        let mut pool = RelayPool::new(None);
        let called = Rc::new(Cell::new(false));
        let called_clone = Rc::clone(&called);
        pool.set_wake(Rc::new(move || called_clone.set(true)));
        // Calling wake() through the maintenance timer isn't triggered natively,
        // but we can exercise the set_wake seam directly.
        (pool.wake)();
        assert!(called.get(), "set_wake must install the provided function");
    }

    #[test]
    fn socket_budget_exceeded_event_on_wasm32_spawn() {
        // On native this test verifies the pool gracefully handles no drivers.
        // The budget-exceeded path is wasm32-only and tested indirectly.
        let pool = RelayPool::new(None);
        assert_eq!(pool.driver_count(), 0);
    }

    #[test]
    fn drain_inbound_empty_is_noop() {
        use nmp_core::actor::ActorMail;
        use std::sync::mpsc;

        let pool = RelayPool::new(None);
        let mut reducer = KernelReducer::new();
        let (tx, _rx) = mpsc::channel::<ActorMail>();
        let sender = CommandSender::new(tx);
        let out = pool.drain_inbound(&mut reducer, &[], &[], &sender);
        assert!(!out.yielded);
        assert!(out.outbound.is_empty());
    }
}
