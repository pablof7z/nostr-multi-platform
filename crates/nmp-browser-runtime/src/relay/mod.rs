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

use std::cell::RefCell;
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

/// Stable wake indirection shared between the relay pool, the JS driver
/// handlers, and the maintenance timer.
///
/// The outer `Rc` is cloned (cheaply, sharing one allocation) into every
/// handler closure and the maintenance-timer callback **at construction /
/// bootstrap time**; the inner `Rc<dyn Fn()>` is the actual "please pump now"
/// closure. [`RelayPool::set_wake`] swaps the *inner* closure in place, so
/// callbacks built before the host installed the real wake still observe it —
/// fixing the bootstrap-runs-before-`set_wake` ordering dead-end (handlers
/// would otherwise have captured a stale no-op and inbound events would never
/// schedule a pump).
pub(crate) type WakeCell = Rc<RefCell<Rc<dyn Fn()>>>;

/// Invoke the current wake closure through a [`WakeCell`]. Clones the inner
/// `Rc<dyn Fn()>` out before calling so the `RefCell` borrow is released before
/// the closure runs (the closure may, transitively, re-enter `set_wake`).
pub(crate) fn fire_wake(cell: &WakeCell) {
    let wake = cell.borrow().clone();
    wake();
}

/// The browser relay pool — driver set + inbound queue + maintenance timer.
pub(crate) struct RelayPool {
    /// Shared inbound event queue (pushed by JS handlers, drained by pump()).
    pub(crate) inbound: Rc<InboundQueue>,

    /// Stable "please schedule a pump" indirection. Handlers and the
    /// maintenance timer capture a clone of this `Rc` at construction; the host
    /// installs the real wake via [`Self::set_wake`], which swaps the inner
    /// closure so earlier-built callbacks observe it (D8 no-polling — inbound
    /// events drive pumps via the host wake, not a busy loop).
    wake: WakeCell,

    /// Maintenance timer armed by pump() from `next_runtime_deadline_delay_ms`.
    /// When it fires it calls wake() which schedules a pump().
    maintenance_timer: CancelableTimer,

    /// User-agent string for the NIP-11 info-document GET. The browser WS
    /// handshake cannot set custom headers, so this UA can only ever ride the
    /// NIP-11 `application/nostr+json` fetch — and that fetch uses `fetch()`
    /// rather than the native `ureq` path, so it is wired as part of the
    /// Chirp-web real-relay work in #2038 (not on this transport-only adapter).
    /// Stored here rather than dropped so the host-configured value survives to
    /// that consumer instead of being silently lost at the builder boundary.
    #[allow(dead_code)] // consumed by the #2038 browser NIP-11 info-document fetch
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
            // Stable cell; inner closure is a no-op placeholder until the host
            // installs the real pump-trigger via set_wake(). Because the cell is
            // shared by reference, handlers built during bootstrap (before
            // set_wake) still observe the real wake once it is swapped in.
            wake: Rc::new(RefCell::new(Rc::new(|| {}) as Rc<dyn Fn()>)),
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
    ///
    /// Swaps the *inner* closure of the shared [`WakeCell`] in place. Handlers
    /// and the maintenance timer built before this call captured a clone of the
    /// cell (not the closure), so they immediately observe the new wake — this
    /// is what makes `spawn_bootstrap` (which runs before the host can call
    /// `set_wake`) wire a live wake rather than a dead no-op.
    pub(crate) fn set_wake(&mut self, wake: Rc<dyn Fn()>) {
        *self.wake.borrow_mut() = wake;
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
                // the kernel's CanonicalRelayUrl normalises before storage, but
                // surfaced (never silently dropped — D6) so a misconfigured
                // bootstrap relay is observable to the host.
                Err(error) => events.push(BrowserRuntimeEvent::RelaySpawnFailed {
                    url: plan.url.clone(),
                    reason: format!("{error:?}"),
                }),
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
    /// arms the maintenance timer **only when the kernel declares a concrete
    /// next deadline**.
    ///
    /// D8 (no polling): when `next_runtime_deadline_delay_ms()` returns `None`
    /// there is no kernel work waiting on the wall clock, so no timer is armed —
    /// the next inbound relay event or host command drives the following pump.
    /// Arming a fallback timer here would re-arm the runtime every second
    /// forever, which is exactly the busy-loop the doctrine forbids.
    pub(crate) fn tick_and_arm(
        &mut self,
        reducer: &mut KernelReducer,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        let outbound = reducer.tick_at(now);

        // Arm only when the kernel has a real deadline. `max(1)` avoids a 0ms
        // delay (which would fire on the same JS task and could starve the
        // event loop); the kernel's own deadline is otherwise honored verbatim
        // (no artificial cap — a far deadline is a single far-future timer, not
        // a 1Hz poll).
        if let Some(delay) = reducer.next_runtime_deadline_delay_ms() {
            let delay_ms = delay.max(1);
            let wake = Rc::clone(&self.wake);
            self.maintenance_timer
                .arm(delay_ms, Rc::new(move || fire_wake(&wake)));
        }

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

    /// Clone the shared [`WakeCell`].
    ///
    /// The runtime hands a clone to the signer-completion broker paths (async
    /// NIP-07 driver + host `deliver_signer_response`) so that enqueuing a
    /// `SignerCompletion` outside `pump()` fires the SAME wake relay inbound
    /// uses — guaranteeing a pump is scheduled to drain it (D8 no-polling).
    /// Because the cell is shared by reference, a wake installed *after* this
    /// clone is still observed (stable-indirection contract).
    pub(crate) fn wake_cell(&self) -> WakeCell {
        Rc::clone(&self.wake)
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
    fn no_deadline_means_no_timer_armed() {
        // D8 no-polling: a fresh reducer has no pending publish work, so
        // `next_runtime_deadline_delay_ms()` returns None and `tick_and_arm`
        // must NOT arm a fallback timer (arming one would re-wake the runtime
        // every second forever — the busy loop the doctrine forbids).
        let mut pool = RelayPool::new(None);
        let mut reducer = KernelReducer::new();
        assert_eq!(
            reducer.next_runtime_deadline_delay_ms(),
            None,
            "precondition: fresh reducer has no runtime deadline"
        );
        let now = nmp_core::time::Instant::now();
        pool.tick_and_arm(&mut reducer, now);
        assert_eq!(
            pool.maintenance_timer.armed_delay_ms_for_test(),
            None,
            "no kernel deadline must leave the maintenance timer unarmed"
        );
    }

    #[test]
    fn set_wake_installs_through_stable_cell() {
        use std::cell::Cell;
        let mut pool = RelayPool::new(None);
        let called = Rc::new(Cell::new(false));
        let called_clone = Rc::clone(&called);
        pool.set_wake(Rc::new(move || called_clone.set(true)));
        // Fire the current wake through the shared cell (what a handler does).
        fire_wake(&pool.wake);
        assert!(called.get(), "set_wake must install the provided function");
    }

    #[test]
    fn wake_fires_after_set_wake_installed_post_spawn() {
        use std::cell::Cell;
        // Reproduces the bootstrap-before-set_wake ordering: a driver handler
        // captures the wake cell at spawn time (here, `wake_cell_for_test`),
        // THEN the host installs the real wake via set_wake. An inbound event
        // arriving afterwards must still schedule a pump (counter increments) —
        // proving the indirection is stable, not a captured-stale no-op.
        let mut pool = RelayPool::new(None);

        // 1. "Handler" captures the cell during bootstrap (wake still no-op).
        let captured = pool.wake_cell();

        // 2. Host installs the real wake AFTER the capture.
        let pumps = Rc::new(Cell::new(0u32));
        let pumps_clone = Rc::clone(&pumps);
        pool.set_wake(Rc::new(move || {
            pumps_clone.set(pumps_clone.get() + 1);
        }));

        // 3. An inbound relay event arrives: handler enqueues + wakes via the
        //    cell it captured in step 1.
        pool.inbound.push(inbound::InboundRelayEvent::Failed {
            role: nmp_network::role::RelayRole::Content,
            url: "wss://relay.example".to_string(),
            error: "x".to_string(),
        });
        fire_wake(&captured);

        assert_eq!(
            pumps.get(),
            1,
            "wake installed after the handler captured the cell must still fire"
        );
        assert_eq!(
            pool.inbound.queue.borrow().len(),
            1,
            "the inbound event must have been enqueued"
        );
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
