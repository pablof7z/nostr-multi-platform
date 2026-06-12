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
//! 3. Runs the `post_tick_drain` hook (PR-4) **after** step 1's borrow is
//!    released. Wasm32 composition roots use this hook to drain the
//!    pending-claim queue without re-entering the reducer while it is
//!    borrowed. If no drain is installed the step is a no-op.
//! 4. **Iff** `dirty` — pushes a snapshot through the registered JS callback
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
/// 3. Runs the post-tick drain hook if installed (PR-4 claim drain).
/// 4. Pushes a snapshot iff `dirty`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn start_tick_interval(
    reducer: Rc<RefCell<KernelReducer>>,
    drivers: Rc<RefCell<Vec<Rc<nmp_network::browser_driver::BrowserRelayDriver>>>>,
    snapshot_callback: Rc<RefCell<Option<js_sys::Function>>>,
    meta: Rc<RefCell<crate::snapshot::RuntimeMeta>>,
    post_tick_drain: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> gloo_timers::callback::Interval {
    gloo_timers::callback::Interval::new(1_000, move || {
        // Step 1: tick. The borrow_mut is scoped to tick_once and drops
        // before this closure continues — the post-tick drain is therefore
        // safe to call here without a RefCell panic.
        let (outbound, dirty) = tick_once(&reducer);
        // Step 2: fan outbound relay frames (canonical fan-out via relay_pool).
        crate::relay_pool::fan_out_outbound(&drivers, &outbound);
        // Step 3: post-tick drain (PR-4). Runs AFTER the reducer borrow
        // from tick_once is fully released, so the drain closure can safely
        // call reducer.borrow_mut() to process queued claim/release requests.
        if let Ok(slot) = post_tick_drain.try_borrow() {
            if let Some(drain) = slot.as_ref() {
                drain();
            }
        }
        // Step 4: push snapshot if state changed.
        if dirty {
            crate::snapshot::push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
        }
    })
}
