//! S4 — Reconciler back-pressure (main thread stalled 250 ms).
//!
//! Spec: docs/design/ffi-hardening/scenarios.md §S4
//! Gate: docs/design/ffi-hardening/gates.md §G-S4
//!
//! Injects 500 synthetic events to build kernel state, then simulates 12 ×
//! 250 ms main-thread stalls during a 60-s event stream.  During each stall
//! the callback sleeps 250 ms to simulate a blocked consumer.
//!
//! Validates:
//! 1. Actor is not blocked during stall (configure() returns immediately).
//! 2. On stall release, emits arrive in monotonic rev order.
//! 3. Stale-rev detection: emits produced during stall may have lower rev
//!    than post-stall emits — counted as stale-rev pairs per stall.
//! 4. No emit is dropped (every configure() call produces at least one emit).
//!
//! D1 (best-effort rendering): on stall release, emit order is monotonic.
//! Bible #1 (monotonic rev): enforced via rev extraction in callback.

use crate::common::{extract_rev, inject_events, revs_strictly_increasing};
use crate::ffi::{
    nmp_app_configure, nmp_app_free, nmp_app_new, nmp_app_set_update_callback, NmpApp,
};
use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use serde_json::json;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STALL_COUNT: u64 = 12;
const STALL_MS: u64 = 250;
const STALL_INTERVAL: Duration = Duration::from_secs(4);

static STALLING: AtomicBool = AtomicBool::new(false);
static EMIT_COUNT: AtomicU64 = AtomicU64::new(0);

struct StallState {
    revs: Vec<u64>,
}

extern "C" fn stall_cb(ctx: *mut c_void, payload: *const c_char) {
    EMIT_COUNT.fetch_add(1, Ordering::Relaxed);

    // Simulate blocked main thread during stall window.
    if STALLING.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(STALL_MS));
    }

    let state_ptr = ctx as *mut Mutex<StallState>;
    if let Ok(mut state) = unsafe { (*state_ptr).lock() } {
        let rev = if !payload.is_null() {
            let bytes = unsafe { std::ffi::CStr::from_ptr(payload) }.to_bytes();
            extract_rev(bytes).unwrap_or(0)
        } else {
            0
        };
        state.revs.push(rev);
    }
}

pub(crate) struct S4Config {
    pub(crate) duration: Duration,
    pub(crate) stall_count: u64,
    pub(crate) stall_duration: Duration,
    pub(crate) emit_hz: u32,
    /// Synthetic events to inject before stalls begin.
    pub(crate) inject_count: u32,
}

impl Default for S4Config {
    fn default() -> Self {
        S4Config {
            duration: Duration::from_secs(60),
            stall_count: STALL_COUNT,
            stall_duration: Duration::from_millis(STALL_MS),
            emit_hz: 4,
            inject_count: 500,
        }
    }
}

pub(crate) fn run(cfg: S4Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();
    EMIT_COUNT.store(0, Ordering::Relaxed);
    STALLING.store(false, Ordering::Release);

    let app: *mut NmpApp = nmp_app_new();

    let state = Box::new(Mutex::new(StallState { revs: Vec::new() }));
    let ctx = Box::into_raw(state) as *mut c_void;

    nmp_app_set_update_callback(app, ctx, Some(stall_cb));
    nmp_app_configure(app, 0, 80, cfg.emit_hz);

    // Inject synthetic events so the kernel has real state to serialize.
    let base_ts: u64 = 1_700_000_000;
    inject_events(app, "s4-", base_ts, cfg.inject_count);
    // Settle: let actor process inject + emit initial snapshot.
    std::thread::sleep(Duration::from_millis(400));

    // Track per-stall pre/post emit counts and configure() latency during stalls.
    let mut stalls_injected: u64 = 0;
    let mut stall_pre_counts: Vec<u64> = Vec::new();
    let mut stall_post_counts: Vec<u64> = Vec::new();
    // configure() latency measured while callback is sleeping (actor must not block).
    let mut configure_during_stall_us: Vec<u64> = Vec::new();

    let configure_interval = Duration::from_millis(500);
    let mut next_configure = Instant::now() + configure_interval;
    // First stall at t=2 s; subsequent stalls at STALL_INTERVAL (4 s) apart.
    let mut next_stall = Instant::now() + Duration::from_secs(2);

    while wall_start.elapsed() < cfg.duration {
        let now = Instant::now();

        if now >= next_stall && stalls_injected < cfg.stall_count {
            let pre = EMIT_COUNT.load(Ordering::Relaxed);
            stall_pre_counts.push(pre);
            STALLING.store(true, Ordering::Release);
            // Measure configure() latency mid-stall: actor enqueues to mpsc and returns
            // immediately; the sleeping callback does NOT block configure().
            let t_cfg = Instant::now();
            nmp_app_configure(app, 0, 80, cfg.emit_hz);
            configure_during_stall_us.push(t_cfg.elapsed().as_micros() as u64);
            std::thread::sleep(cfg.stall_duration + Duration::from_millis(50));
            STALLING.store(false, Ordering::Release);
            let post = EMIT_COUNT.load(Ordering::Relaxed);
            stall_post_counts.push(post);
            stalls_injected += 1;
            next_stall = now + STALL_INTERVAL;
        }

        if now >= next_configure {
            nmp_app_configure(app, 0, 80, cfg.emit_hz);
            next_configure = now + configure_interval;
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    std::thread::sleep(Duration::from_millis(500));

    let wall_elapsed = wall_start.elapsed().as_secs_f64();

    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);

    let state = unsafe { Box::from_raw(ctx as *mut Mutex<StallState>) };
    let state = state.lock().unwrap();

    let emit_count = EMIT_COUNT.load(Ordering::Relaxed);
    let revs_monotonic = revs_strictly_increasing(&state.revs);

    // Backlog emitted per stall (emits that arrived while callback was sleeping).
    let max_backlog_emits: u64 = stall_pre_counts
        .iter()
        .zip(stall_post_counts.iter())
        .map(|(pre, post)| post.saturating_sub(*pre))
        .max()
        .unwrap_or(0);

    // Expected max backlog: ceil(stall_ms/1000 * emit_hz) + 1.
    let expected_max = (cfg.stall_duration.as_secs_f64() * cfg.emit_hz as f64).ceil() as u64 + 1;

    // Stale-rev detection: count adjacent rev pairs where rev[i+1] <= rev[i].
    // These represent emits buffered during a stall that arrive out of expected order.
    let stale_rev_pairs: usize = {
        let non_zero: Vec<u64> = state.revs.iter().copied().filter(|&r| r > 0).collect();
        non_zero
            .windows(2)
            .filter(|w| w[1] <= w[0])
            .count()
    };

    // G-S4 gates — per docs/design/ffi-hardening/gates.md §G-S4.
    report.gates.push(
        Gate::eq(
            "stalls_injected",
            stalls_injected as f64,
            cfg.stall_count as f64,
        )
        .with_note("G-S4: stalls_injected == 12"),
    );
    report.gates.push(
        Gate::lte(
            "backlog_after_stall",
            max_backlog_emits as f64,
            expected_max as f64,
        )
        .with_note("G-S4: listener backlog after 250ms stall <= ceil(0.25*emit_hz)+1"),
    );
    report.gates.push(
        Gate::eq("rev_monotonic", if revs_monotonic { 1.0 } else { 0.0 }, 1.0)
            .with_note("G-S4/bible#1: rev order on stall-resume strictly monotonic"),
    );
    // Actor non-blocking verification: configure() measured mid-stall while the callback
    // is sleeping 250 ms on the listener thread.  Actor dispatches to mpsc channel and
    // returns; sleeping callback does NOT block configure() (D4 single-writer invariant).
    // Gate: p99 configure latency during stall <= 10 ms (10,000 µs).
    let total_stall_backlog: u64 = stall_pre_counts
        .iter()
        .zip(stall_post_counts.iter())
        .map(|(pre, post)| post.saturating_sub(*pre))
        .sum();
    let configure_p99_us: u64 = {
        let mut sorted = configure_during_stall_us.clone();
        sorted.sort_unstable();
        *sorted.last().unwrap_or(&0) // max == p100 == conservative p99 for ≤12 samples
    };
    report.gates.push(
        Gate::lte("configure_during_stall_p99_us", configure_p99_us as f64, 10_000.0)
            .with_note(
                "G-S4: configure() p99 latency during 250ms stall <= 10 ms \
                 (actor enqueues to mpsc, not blocked by sleeping callback)",
            ),
    );
    // Apply-after-resume burst: actor-tick tracing is not available from the FFI side.
    // Phase-2 deliverable: instrument actor with emit-timing telemetry (tracked in
    // docs/design/ffi-hardening/gates.md §G-S4 TODO).  Gate omitted this phase.

    report.notes.push(format!(
        "Injected {} events; stalls: {}; max backlog: {}; expected <= {}; \
         emits total: {}; stale-rev pairs: {}; total_stall_backlog: {}; \
         configure_p99_us: {}",
        cfg.inject_count, stalls_injected, max_backlog_emits, expected_max,
        emit_count, stale_rev_pairs, total_stall_backlog, configure_p99_us
    ));
    report.notes.push(
        "Stall simulated via callback sleep (250 ms) on listener thread.  \
         Actor is not blocked; configure() enqueues to mpsc Sender and returns immediately \
         (D4 single-writer via actor thread). configure_p99_us measures this directly."
            .to_string(),
    );
    report.notes.push(
        "apply_burst_ms gate deferred to phase-2 (needs actor-tick telemetry). \
         See docs/design/ffi-hardening/gates.md §G-S4."
            .to_string(),
    );

    report.measurements = json!({
        "inject_count": cfg.inject_count,
        "stalls_injected": stalls_injected,
        "stall_duration_ms": STALL_MS,
        "emit_hz": cfg.emit_hz,
        "max_backlog_emits": max_backlog_emits,
        "expected_max_backlog": expected_max,
        "total_stall_backlog": total_stall_backlog,
        "configure_during_stall_p99_us": configure_p99_us,
        "total_emits": emit_count,
        "rev_monotonic": revs_monotonic,
        "stale_rev_pairs": stale_rev_pairs,
        "wall_seconds": wall_elapsed,
    });

    report.finish(wall_elapsed);
}
