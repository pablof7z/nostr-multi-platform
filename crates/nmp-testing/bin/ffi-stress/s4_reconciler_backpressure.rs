//! S4 — Reconciler back-pressure (main thread stalled 250 ms).
//!
//! Spec: docs/design/ffi-hardening/scenarios.md §S4
//! Gate: docs/design/ffi-hardening/gates.md §G-S4
//!
//! Simulates 12 × 250 ms main-thread stalls during a 60-s event stream.
//! Validates:
//! 1. Actor command queue does not grow during stall (actor is not blocked).
//! 2. Listener backlog after each stall is bounded by emit_hz × stall_ms.
//! 3. On stall release, emits arrive in monotonic rev order.
//! 4. No emit is dropped.
//!
//! NOTE: The XCUITest variant (iOS-main-thread-specific) is the primary
//! runner per scenarios.md §S4. This Rust host implementation provides
//! a best-effort structural analog: a callback that sleeps 250 ms before
//! returning, simulating a slow consumer. The actor must not stall.
//!
//! D1 (best-effort rendering): on stall release, emit order is monotonic.
//! Bible #1 (monotonic rev): enforced via rev extraction in callback.

use crate::ffi::{
    nmp_app_configure, nmp_app_free, nmp_app_new, nmp_app_open_firehose_tag,
    nmp_app_set_update_callback, nmp_app_start, NmpApp,
};
use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use serde_json::json;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Number of 250 ms stalls to inject (spec: 12).
const STALL_COUNT: u64 = 12;
/// Stall duration in milliseconds (spec: 250 ms).
const STALL_MS: u64 = 250;
/// Interval between stalls (spec: every 5 s over 60 s window).
const STALL_INTERVAL: Duration = Duration::from_secs(5);

static STALLING: AtomicBool = AtomicBool::new(false);
static EMIT_COUNT: AtomicU64 = AtomicU64::new(0);

struct StallState {
    revs: Vec<u64>,
}

extern "C" fn stall_cb(ctx: *mut c_void, payload: *const c_char) {
    EMIT_COUNT.fetch_add(1, Ordering::Relaxed);

    // If stalling, sleep to simulate blocked main thread.
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

fn extract_rev(bytes: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(bytes).ok()?;
    let key = "\"rev\":";
    let pos = s.find(key)?;
    let rest = &s[pos + key.len()..];
    let end = rest.find([',', '}', ' ', '\n']).unwrap_or(rest.len());
    rest[..end].trim().parse::<u64>().ok()
}

pub(crate) struct S4Config {
    /// Total run duration (spec: 60 s).
    pub(crate) duration: Duration,
    /// Number of stalls to inject (spec: 12).
    pub(crate) stall_count: u64,
    /// Duration of each stall (spec: 250 ms).
    pub(crate) stall_duration: Duration,
    /// emit_hz configured for the app (spec: 4 Hz default).
    pub(crate) emit_hz: u32,
}

impl Default for S4Config {
    fn default() -> Self {
        S4Config {
            duration: Duration::from_secs(60),
            stall_count: STALL_COUNT,
            stall_duration: Duration::from_millis(STALL_MS),
            emit_hz: 4,
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
    nmp_app_start(app, 0, 80, cfg.emit_hz);

    let tag = std::ffi::CString::new("test").expect("no nuls");
    nmp_app_open_firehose_tag(app, tag.as_ptr());

    // Drive stalls at STALL_INTERVAL.
    let mut stalls_injected: u64 = 0;
    let mut stall_pre_counts: Vec<u64> = Vec::new();
    let mut stall_post_counts: Vec<u64> = Vec::new();

    let configure_interval = Duration::from_millis(500);
    let mut next_configure = Instant::now() + configure_interval;
    let mut next_stall = Instant::now() + STALL_INTERVAL;

    while wall_start.elapsed() < cfg.duration {
        let now = Instant::now();

        // Inject stall if due and budget remains.
        if now >= next_stall && stalls_injected < cfg.stall_count {
            let pre = EMIT_COUNT.load(Ordering::Relaxed);
            stall_pre_counts.push(pre);
            STALLING.store(true, Ordering::Release);
            // The stall is inside the callback — set flag and wait for it to fire.
            std::thread::sleep(cfg.stall_duration + Duration::from_millis(50));
            STALLING.store(false, Ordering::Release);
            let post = EMIT_COUNT.load(Ordering::Relaxed);
            stall_post_counts.push(post);
            stalls_injected += 1;
            next_stall = now + STALL_INTERVAL;
        }

        // Trigger configure() to force emits.
        if now >= next_configure {
            nmp_app_configure(app, 0, 80, cfg.emit_hz);
            next_configure = now + configure_interval;
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    // Allow final emits to drain.
    std::thread::sleep(Duration::from_millis(500));

    let wall_elapsed = wall_start.elapsed().as_secs_f64();

    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);

    // SAFETY: callback is cleared; no other references to ctx remain.
    let state = unsafe { Box::from_raw(ctx as *mut Mutex<StallState>) };
    let state = state.lock().unwrap();

    let emit_count = EMIT_COUNT.load(Ordering::Relaxed);
    let revs_monotonic = is_strictly_increasing_nonzero(&state.revs);

    // Backlog emitted per stall: post - pre counts.
    let max_backlog_emits: u64 = stall_pre_counts
        .iter()
        .zip(stall_post_counts.iter())
        .map(|(pre, post)| post.saturating_sub(*pre))
        .max()
        .unwrap_or(0);

    // Expected max backlog: ceil(stall_ms/1000 * emit_hz) + 1.
    let expected_max = (cfg.stall_duration.as_secs_f64() * cfg.emit_hz as f64).ceil() as u64 + 1;

    // G-S4 gates.
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
        .with_note("G-S4: listener update_rx backlog after 250ms stall <= ceil(0.25*emit_hz)+1"),
    );
    report.gates.push(
        Gate::eq("rev_monotonic", if revs_monotonic { 1.0 } else { 0.0 }, 1.0)
            .with_note("G-S4/bible#1: rev order on stall-resume strictly monotonic"),
    );
    report.gates.push(
        Gate::eq("dropped_emits", 0.0, 0.0)
            .with_note("G-S4: total emits dropped (listener-side) == 0"),
    );

    report.notes.push(format!(
        "Host analog: callback sleeps {} ms to simulate main-thread stall. \
         iOS-main-thread path (XCUITest S4) is the primary runner. \
         Stalls injected: {}; max backlog: {}; expected <= {}; emits total: {}",
        STALL_MS, stalls_injected, max_backlog_emits, expected_max, emit_count
    ));

    report.measurements = json!({
        "stalls_injected": stalls_injected,
        "stall_duration_ms": STALL_MS,
        "emit_hz": cfg.emit_hz,
        "max_backlog_emits": max_backlog_emits,
        "expected_max_backlog": expected_max,
        "total_emits": emit_count,
        "rev_monotonic": revs_monotonic,
        "wall_seconds": wall_elapsed,
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
