//! PR-2 — periodic 1 Hz tick driver for the wasm32 runtime.
//!
//! # Shared core: `tick_once`
//!
//! [`tick_once`] is **not** `cfg(wasm32)`-gated: it calls `KernelReducer::tick()`
//! (which drains the publish engine and the subscription lifecycle outbound)
//! and reads back the `changed_since_emit` flag. Both the wasm32 timer closure
//! AND the native [`super::runtime::WasmRuntime::tick_for_test`] helper call
//! this function, so the dirty-flag coalescing logic is exercised on native CI
//! even though the gloo-timers closure itself only exists on wasm32.
//!
//! # wasm32 timer: `start_tick_interval`
//!
//! [`start_tick_interval`] (wasm32-only) constructs a
//! `gloo_timers::callback::Interval` that fires once per second on the JS event
//! loop. Each tick:
//!
//! 1. Calls `tick_once` — borrows, ticks, reads dirty flag, drops borrow.
//! 2. Fans the outbound batch through `relay_pool::fan_out_outbound` to live
//!    relay drivers (the single canonical fan-out that uses `.filter()` so
//!    every matching driver on a `"both"`-role URL receives the frame).
//! 3. **Iff** `dirty` — pushes a snapshot through the registered JS callback
//!    (`snapshot::push_snapshot_if_callback`). Idle ticks with no state change
//!    skip the push, avoiding spurious JS-heap allocations and upstream
//!    re-renders.
//!
//! Dropping the returned `Interval` cancels the underlying `setInterval` call;
//! `WasmRuntime::stop()` sets `tick_interval = None` before tearing down relay
//! drivers.

use std::cell::RefCell;
use std::rc::Rc;

use nmp_core::{KernelReducer, OutboundMessage};

/// Drive one tick cycle and return `(outbound, dirty)`.
///
/// `dirty` is the value of `KernelReducer::changed_since_emit()` sampled
/// **after** `tick()` returns — the wasm32 timer uses it to decide whether to
/// push a snapshot (`true` → push; `false` → skip). The borrow on `reducer`
/// is fully released before the function returns, so callers can safely
/// re-borrow for the snapshot push without a `RefCell` panic.
pub(crate) fn tick_once(
    reducer: &Rc<RefCell<KernelReducer>>,
) -> (Vec<OutboundMessage>, bool) {
    let mut r = reducer.borrow_mut();
    let outbound = r.tick();
    let dirty = r.changed_since_emit();
    (outbound, dirty)
}

/// Start the 1 Hz periodic tick interval. Returns a
/// `gloo_timers::callback::Interval` whose `Drop` impl cancels the JS
/// `setInterval`. Wasm32-only.
///
/// The closure captures `Rc` handles — not references — so the timer can
/// outlive any particular borrow window on `WasmRuntime`. Each fire:
///
/// 1. Calls `tick_once` (borrows reducer, ticks, drops borrow).
/// 2. Fans outbound to relay drivers.
/// 3. Pushes a snapshot iff `dirty`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn start_tick_interval(
    reducer: Rc<RefCell<KernelReducer>>,
    drivers: Rc<RefCell<Vec<Rc<nmp_network::browser_driver::BrowserRelayDriver>>>>,
    snapshot_callback: Rc<RefCell<Option<js_sys::Function>>>,
    meta: Rc<RefCell<crate::snapshot::RuntimeMeta>>,
) -> gloo_timers::callback::Interval {
    gloo_timers::callback::Interval::new(1_000, move || {
        let (outbound, dirty) = tick_once(&reducer);
        crate::relay_pool::fan_out_outbound(&drivers, &outbound);
        if dirty {
            crate::snapshot::push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
        }
    })
}
