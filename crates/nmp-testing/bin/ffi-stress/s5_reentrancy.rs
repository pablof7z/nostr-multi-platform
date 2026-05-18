//! S5 — Reentrancy (dispatch from inside reconciler callback).
//!
//! Spec: docs/design/ffi-hardening/scenarios.md §S5
//! Gate: docs/design/ffi-hardening/gates.md §G-S5
//!
//! Registers a callback that, on every emit, immediately dispatches
//! `open_author` for a test pubkey from the listener thread.
//! Sustains for 30 s with configure() driving emits at ~4 Hz.
//!
//! Key invariants:
//! - Zero deadlocks (external 5 s watchdog thread detects hangs).
//! - Reentrant dispatch: nmp_app_open_author enqueues to actor's mpsc Sender
//!   (Send+Sync); the send never blocks or re-locks kernel state (bible #3).
//! - Rev monotonicity after each emit.
//! - No dispatch loss.
//!
//! D4 (single writer): reentrant dispatch enqueues to actor; callback does
//!    not mutate kernel state directly.
//! Bible #3 (fire-and-forget): send inside callback returns immediately.

use crate::common::{extract_rev, inject_signed_events, revs_strictly_increasing};
use crate::ffi::{
    nmp_app_configure, nmp_app_free, nmp_app_new, nmp_app_open_author, nmp_app_set_update_callback,
    test_pubkeys, NmpApp,
};
use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use serde_json::json;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 5 s watchdog: if an emit callback is still in flight after this window, it's a deadlock.
const WATCHDOG_TIMEOUT_MS: u64 = 5_000;

/// Shared epoch: set once at scenario start; all timestamps are ms-since-epoch.
/// 0 in CB_IN_FLIGHT_TS_MS means no callback is in flight.
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// Non-zero while a callback is in flight: stores epoch-relative ms of callback entry.
/// Reset to 0 on callback return.  Watchdog: fires if non-zero AND age > WATCHDOG_TIMEOUT_MS.
static CB_IN_FLIGHT_TS_MS: AtomicU64 = AtomicU64::new(0);

static EMIT_COUNT: AtomicU64 = AtomicU64::new(0);
static REENTRANT_DISPATCHES: AtomicU64 = AtomicU64::new(0);
/// Set by watchdog if a deadlock is detected; scenario treats as fatal.
static WATCHDOG_FIRED: AtomicBool = AtomicBool::new(false);
/// Set by run() when the scenario loop finishes; watchdog exits on this.
static SCENARIO_DONE: AtomicBool = AtomicBool::new(false);

struct ReentryState {
    app_usize: usize,
    pubkeys: Vec<std::ffi::CString>,
    cb_latencies_ns: Vec<u64>,
    revs: Vec<u64>,
}

extern "C" fn reentrant_cb(ctx: *mut c_void, payload: *const c_char) {
    let t_entry_ms = EPOCH
        .get()
        .map(|e| e.elapsed().as_millis() as u64)
        .unwrap_or(0);
    // Mark callback in-flight: store epoch-relative entry timestamp (non-zero).
    CB_IN_FLIGHT_TS_MS.store(t_entry_ms.max(1), Ordering::Release);
    let t_start = Instant::now();

    let emit_n = EMIT_COUNT.fetch_add(1, Ordering::Relaxed);

    let state_ptr = ctx as *mut Mutex<ReentryState>;
    let (app_ptr, pk_cstr, rev) = {
        let Ok(state) = (unsafe { (*state_ptr).lock() }) else {
            CB_IN_FLIGHT_TS_MS.store(0, Ordering::Release);
            return;
        };
        let pk = state.pubkeys[emit_n as usize % state.pubkeys.len()].clone();
        let rev = if !payload.is_null() {
            let bytes = unsafe { std::ffi::CStr::from_ptr(payload) }.to_bytes();
            extract_rev(bytes).unwrap_or(0)
        } else {
            0
        };
        (state.app_usize as *mut NmpApp, pk, rev)
    };

    // Reentrant dispatch: enqueues to actor mpsc channel (fire-and-forget, bible #3).
    // Must not block: the actor's Sender::send() is O(1) non-blocking.
    nmp_app_open_author(app_ptr, pk_cstr.as_ptr());
    REENTRANT_DISPATCHES.fetch_add(1, Ordering::Relaxed);

    let total_ns = t_start.elapsed().as_nanos() as u64;

    // Mark callback complete: reset in-flight flag to 0.
    CB_IN_FLIGHT_TS_MS.store(0, Ordering::Release);

    if let Ok(mut state) = unsafe { (*state_ptr).lock() } {
        state.cb_latencies_ns.push(total_ns);
        state.revs.push(rev);
    }
}

pub(crate) struct S5Config {
    pub(crate) duration: Duration,
    pub(crate) events_per_sec: u32,
    /// Synthetic events to inject before dispatch loop.
    pub(crate) inject_count: u32,
}

impl Default for S5Config {
    fn default() -> Self {
        S5Config {
            duration: Duration::from_secs(30),
            events_per_sec: 50,
            inject_count: 200,
        }
    }
}

pub(crate) fn run(cfg: S5Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();
    EMIT_COUNT.store(0, Ordering::Relaxed);
    REENTRANT_DISPATCHES.store(0, Ordering::Relaxed);
    WATCHDOG_FIRED.store(false, Ordering::Release);
    SCENARIO_DONE.store(false, Ordering::Release);
    // Initialise epoch; CB_IN_FLIGHT_TS_MS = 0 means no callback in flight.
    let _ = EPOCH.set(Instant::now());
    CB_IN_FLIGHT_TS_MS.store(0, Ordering::Release);

    let app: *mut NmpApp = nmp_app_new();
    let pubkeys = test_pubkeys(10);
    let state = Box::new(Mutex::new(ReentryState {
        app_usize: app as usize,
        pubkeys,
        cb_latencies_ns: Vec::new(),
        revs: Vec::new(),
    }));
    let ctx = Box::into_raw(state) as *mut c_void;

    nmp_app_set_update_callback(app, ctx, Some(reentrant_cb));
    nmp_app_configure(app, 0, 80, 4);

    // Inject real Schnorr-signed events via try_from_raw verify path.
    // S5 uses full verify (D7: 200 events ~6-10 ms; acceptable for setup).
    inject_signed_events(app, 1_700_000_000, cfg.inject_count);

    // Spawn external watchdog BEFORE the dispatch loop.
    //
    // Protocol:
    //   - CB_IN_FLIGHT_TS_MS == 0  → no callback in flight (safe).
    //   - CB_IN_FLIGHT_TS_MS > 0   → callback entered at that epoch-ms; watchdog fires
    //                                 if (epoch.elapsed_ms - stored_ms) > WATCHDOG_TIMEOUT_MS.
    //
    // This is correct because EPOCH is monotonic and shared between callback and watchdog;
    // the gap is the true wall-time the callback has been running.
    let watchdog = std::thread::spawn(move || {
        loop {
            if SCENARIO_DONE.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            let in_flight_ms = CB_IN_FLIGHT_TS_MS.load(Ordering::Acquire);
            if in_flight_ms == 0 {
                continue; // no callback in flight
            }
            let now_ms = EPOCH
                .get()
                .map(|e| e.elapsed().as_millis() as u64)
                .unwrap_or(0);
            let age_ms = now_ms.saturating_sub(in_flight_ms);
            if age_ms > WATCHDOG_TIMEOUT_MS {
                WATCHDOG_FIRED.store(true, Ordering::Release);
                eprintln!(
                    "ffi-stress S5: DEADLOCK detected — callback in flight for {age_ms} ms \
                     (threshold {WATCHDOG_TIMEOUT_MS} ms). Terminating."
                );
                std::process::exit(1);
            }
        }
    });

    let interval = Duration::from_nanos(1_000_000_000 / cfg.events_per_sec.max(1) as u64);
    let mut next_tick = Instant::now();

    while wall_start.elapsed() < cfg.duration {
        nmp_app_configure(app, 0, 80, 4);
        next_tick += interval;
        if let Some(sleep) = next_tick.checked_duration_since(Instant::now()) {
            std::thread::sleep(sleep);
        }
    }

    // Grace period for reentrant dispatches to drain.
    std::thread::sleep(Duration::from_millis(500));

    let wall_elapsed = wall_start.elapsed().as_secs_f64();

    // Signal watchdog to exit before teardown.
    SCENARIO_DONE.store(true, Ordering::Release);
    let _ = watchdog.join();

    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);

    let state = unsafe { Box::from_raw(ctx as *mut Mutex<ReentryState>) };
    let state = state.lock().unwrap();

    let emit_count = EMIT_COUNT.load(Ordering::Relaxed);
    let reentrant_count = REENTRANT_DISPATCHES.load(Ordering::Relaxed);
    let watchdog_fired = WATCHDOG_FIRED.load(Ordering::Relaxed);

    let avg_cb_ns = if state.cb_latencies_ns.is_empty() {
        0.0
    } else {
        state.cb_latencies_ns.iter().sum::<u64>() as f64 / state.cb_latencies_ns.len() as f64
    };
    let avg_cb_ms = avg_cb_ns / 1_000_000.0;

    let revs_monotonic = revs_strictly_increasing(&state.revs);
    let dispatch_loss = emit_count.saturating_sub(reentrant_count);
    let deadlock_count: u64 = if watchdog_fired { 1 } else { 0 };

    // G-S5 gates — per docs/design/ffi-hardening/gates.md §G-S5.
    report.gates.push(
        Gate::eq("deadlocks", deadlock_count as f64, 0.0)
            .with_note("G-S5: zero deadlocks (5 s external watchdog; epoch-absolute timestamps)"),
    );
    report.gates.push(
        Gate::gte("reentrant_dispatches", reentrant_count as f64, 100.0)
            .with_note("G-S5: dispatch-from-callback emits processed >= 100 over 30 s"),
    );
    report.gates.push(
        Gate::eq("rev_monotonic", if revs_monotonic { 1.0 } else { 0.0 }, 1.0)
            .with_note("G-S5: out-of-order callback->dispatch pairs == 0 (rev monotonic)"),
    );
    report.gates.push(
        Gate::lte("avg_cb_ms", avg_cb_ms, 2.0)
            .with_note("G-S5: listener thread CPU per emit avg <= 2 ms"),
    );
    report.gates.push(
        Gate::eq("dispatch_loss", dispatch_loss as f64, 0.0)
            .with_note("G-S5: dispatch loss == 0"),
    );

    report.notes.push(format!(
        "Emits: {emit_count}; reentrant dispatches: {reentrant_count}; \
         deadlocks: {deadlock_count}; avg callback: {avg_cb_ms:.3} ms"
    ));
    report.notes.push(
        "External watchdog: shared OnceLock<Instant> epoch; CB_IN_FLIGHT_TS_MS=0 means \
         idle; non-zero stores epoch-relative entry ms. Watchdog fires exit(1) when \
         (epoch.elapsed_ms - entry_ms) > 5000 — correctly measures wall-time in flight."
            .to_string(),
    );
    report.notes.push(
        "Reentrant dispatch is fire-and-forget (bible #3): nmp_app_open_author enqueues \
         to actor mpsc channel; does not block listener thread or re-lock any mutex."
            .to_string(),
    );

    report.measurements = json!({
        "inject_count": cfg.inject_count,
        "emit_count": emit_count,
        "reentrant_dispatches": reentrant_count,
        "deadlocks": deadlock_count,
        "dispatch_loss": dispatch_loss,
        "avg_cb_ms": avg_cb_ms,
        "rev_monotonic": revs_monotonic,
        "wall_seconds": wall_elapsed,
        "watchdog_fired": watchdog_fired,
    });

    report.finish(wall_elapsed);
}
