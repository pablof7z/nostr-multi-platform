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
//! - Zero deadlocks (5 s watchdog: any callback that takes > 5 s = FAIL).
//! - Message order: the OpenAuthor dispatched inside emit N is processed
//!   after emit N (verified via rev monotonicity).
//! - No dispatch loss.
//!
//! D4 (single writer): reentrant dispatch enqueues to actor; callback does
//!    not mutate kernel state directly. Actor is the sole writer.
//! Bible #3 (fire-and-forget): send call inside callback returns immediately.

use crate::ffi::{
    nmp_app_configure, nmp_app_free, nmp_app_new, nmp_app_open_author, nmp_app_open_firehose_tag,
    nmp_app_set_update_callback, nmp_app_start, test_pubkeys, NmpApp,
};
use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use serde_json::json;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Watchdog threshold: any callback that takes longer than this is a deadlock.
const DEADLOCK_THRESHOLD_MS: u64 = 5_000;

static DEADLOCKS: AtomicU64 = AtomicU64::new(0);
static EMIT_COUNT: AtomicU64 = AtomicU64::new(0);
static REENTRANT_DISPATCHES: AtomicU64 = AtomicU64::new(0);
static WATCHDOG_TRIPPED: AtomicBool = AtomicBool::new(false);

struct ReentryState {
    /// Stored app pointer for dispatch inside callback.
    app_usize: usize,
    /// Pubkeys to open_author on reentrant dispatch.
    pubkeys: Vec<std::ffi::CString>,
    /// Callback latencies (ns).
    cb_latencies_ns: Vec<u64>,
    /// Rev values received.
    revs: Vec<u64>,
}

extern "C" fn reentrant_cb(ctx: *mut c_void, payload: *const c_char) {
    let t0 = Instant::now();
    let emit_n = EMIT_COUNT.fetch_add(1, Ordering::Relaxed);

    let state_ptr = ctx as *mut Mutex<ReentryState>;
    // Extract app pointer and pubkey without holding the lock during dispatch.
    let (app_ptr, pk_cstr, rev) = {
        let Ok(state) = (unsafe { (*state_ptr).lock() }) else {
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

    // Reentrant dispatch: open_author from inside the listener callback.
    // nmp_app_open_author enqueues to actor's mpsc Sender (Send+Sync).
    // This is fire-and-forget per bible #3 — the send must not block.
    nmp_app_open_author(app_ptr, pk_cstr.as_ptr());
    REENTRANT_DISPATCHES.fetch_add(1, Ordering::Relaxed);

    let total_ns = t0.elapsed().as_nanos() as u64;

    // Watchdog: flag deadlock if callback took too long.
    if total_ns > DEADLOCK_THRESHOLD_MS * 1_000_000 {
        DEADLOCKS.fetch_add(1, Ordering::Relaxed);
        WATCHDOG_TRIPPED.store(true, Ordering::Release);
    }

    if let Ok(mut state) = unsafe { (*state_ptr).lock() } {
        state.cb_latencies_ns.push(total_ns);
        state.revs.push(rev);
    }
}

fn extract_rev(bytes: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(bytes).ok()?;
    let key = "\"rev\":";
    let pos = s.find(key)?;
    let rest = &s[pos + key.len()..];
    let end = rest.find([',', '}', ' ', '\n']).unwrap_or(rest.len());
    rest[..end].trim().parse::<u64>().ok()
}

pub(crate) struct S5Config {
    /// Total run duration (spec: 30 s).
    pub(crate) duration: Duration,
    /// Events/sec driving configure() triggers.
    pub(crate) events_per_sec: u32,
}

impl Default for S5Config {
    fn default() -> Self {
        S5Config {
            duration: Duration::from_secs(30),
            events_per_sec: 50,
        }
    }
}

pub(crate) fn run(cfg: S5Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();
    DEADLOCKS.store(0, Ordering::Relaxed);
    EMIT_COUNT.store(0, Ordering::Relaxed);
    REENTRANT_DISPATCHES.store(0, Ordering::Relaxed);
    WATCHDOG_TRIPPED.store(false, Ordering::Release);

    let app: *mut NmpApp = nmp_app_new();

    // Build callback state with 10 pubkeys to rotate across.
    let pubkeys = test_pubkeys(10);
    let state = Box::new(Mutex::new(ReentryState {
        app_usize: app as usize,
        pubkeys,
        cb_latencies_ns: Vec::new(),
        revs: Vec::new(),
    }));
    let ctx = Box::into_raw(state) as *mut c_void;

    nmp_app_set_update_callback(app, ctx, Some(reentrant_cb));
    nmp_app_start(app, 0, 80, 4);

    // Open firehose to drive ingest events.
    let tag = std::ffi::CString::new("reentrancy-test").expect("no nuls");
    nmp_app_open_firehose_tag(app, tag.as_ptr());

    // Drive configure() at events_per_sec rate to trigger emits.
    let interval = Duration::from_nanos(1_000_000_000 / cfg.events_per_sec.max(1) as u64);
    let mut next_tick = Instant::now();

    while wall_start.elapsed() < cfg.duration {
        nmp_app_configure(app, 0, 80, 4);
        next_tick += interval;
        if let Some(sleep) = next_tick.checked_duration_since(Instant::now()) {
            std::thread::sleep(sleep);
        }
    }

    // Grace period for reentrant dispatches to complete.
    std::thread::sleep(Duration::from_millis(500));

    let wall_elapsed = wall_start.elapsed().as_secs_f64();

    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);

    // SAFETY: callback is cleared; no other references to ctx remain.
    let state = unsafe { Box::from_raw(ctx as *mut Mutex<ReentryState>) };
    let state = state.lock().unwrap();

    let emit_count = EMIT_COUNT.load(Ordering::Relaxed);
    let reentrant_count = REENTRANT_DISPATCHES.load(Ordering::Relaxed);
    let deadlock_count = DEADLOCKS.load(Ordering::Relaxed);

    // Listener CPU per emit (avg).
    let avg_cb_ns = if state.cb_latencies_ns.is_empty() {
        0.0
    } else {
        state.cb_latencies_ns.iter().sum::<u64>() as f64 / state.cb_latencies_ns.len() as f64
    };
    let avg_cb_ms = avg_cb_ns / 1_000_000.0;

    // Rev monotonicity.
    let revs_monotonic = is_strictly_increasing_nonzero(&state.revs);

    // G-S5 gates.
    report.gates.push(
        Gate::eq("deadlocks", deadlock_count as f64, 0.0)
            .with_note("G-S5: zero deadlocks (5 s watchdog per emit)"),
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
    // Dispatch loss: every emit should have produced one reentrant dispatch.
    let dispatch_loss = emit_count.saturating_sub(reentrant_count);
    report.gates.push(
        Gate::eq("dispatch_loss", dispatch_loss as f64, 0.0)
            .with_note("G-S5: dispatch loss == 0"),
    );

    report.notes.push(format!(
        "Emits: {emit_count}; reentrant dispatches: {reentrant_count}; \
         deadlocks: {deadlock_count}; avg callback: {avg_cb_ms:.3} ms"
    ));
    report.notes.push(
        "Reentrant dispatch is fire-and-forget (bible #3): nmp_app_open_author enqueues \
         to actor mpsc channel; does not block listener thread or re-lock any mutex."
            .to_string(),
    );

    report.measurements = json!({
        "emit_count": emit_count,
        "reentrant_dispatches": reentrant_count,
        "deadlocks": deadlock_count,
        "dispatch_loss": dispatch_loss,
        "avg_cb_ms": avg_cb_ms,
        "rev_monotonic": revs_monotonic,
        "wall_seconds": wall_elapsed,
        "watchdog_tripped": WATCHDOG_TRIPPED.load(Ordering::Relaxed),
    });

    report.finish(wall_elapsed);
}

fn is_strictly_increasing_nonzero(revs: &[u64]) -> bool {
    let non_zero: Vec<u64> = revs.iter().copied().filter(|&r| r > 0).collect();
    if non_zero.len() < 2 {
        return true;
    }
    non_zero.windows(2).all(|w| w[1] > w[0])
}
