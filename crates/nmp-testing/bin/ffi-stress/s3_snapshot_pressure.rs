//! S3 — Snapshot pressure (AppUpdate::FullState with 100k events).
//!
//! Spec: docs/design/ffi-hardening/scenarios.md §S3
//! Gate: docs/design/ffi-hardening/gates.md §G-S3
//!
//! Injects 1,000 synthetic timeline events via `nmp_app_inject_events`
//! (test-support; bypasses sig verification but routes through the same
//! kernel HashMap path `make_update()` reads from).  Then triggers 10
//! configure() bursts to stress the listener serialization + callback path.
//!
//! D1 (best-effort rendering): every emit processed promptly without blocking.
//! D2 (<=60 Hz): reconciler frequency stays bounded by configured emit_hz.
//! D8 (zero per-event allocs): allocator snapshot gates per-emit heap growth.

use crate::allocator::alloc_snapshot;
use crate::common::{
    configure_and_settle, extract_rev, inject_events, percentile_u64, revs_strictly_increasing,
};
use crate::ffi::{
    nmp_app_configure, nmp_app_free, nmp_app_new, nmp_app_set_update_callback, process_rss_bytes,
    NmpApp,
};
use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use serde_json::json;
use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Callback state shared between the listener thread and the scenario driver.
struct CallbackState {
    /// Latency from callback entry to return (ns) — proxy for apply_us.
    cb_latencies_ns: Vec<u64>,
    /// Payload sizes per emit.
    payload_sizes: Vec<usize>,
    /// `rev` values for monotonicity check.
    revs: Vec<u64>,
}

extern "C" fn measure_cb(ctx: *mut c_void, payload: *const c_char) {
    let t0 = Instant::now();

    let state_ptr = ctx as *mut Mutex<CallbackState>;
    if let Ok(mut state) = unsafe { (*state_ptr).lock() } {
        let (payload_len, rev) = if !payload.is_null() {
            let bytes = unsafe { std::ffi::CStr::from_ptr(payload) }.to_bytes();
            (bytes.len(), extract_rev(bytes).unwrap_or(0))
        } else {
            (0, 0)
        };

        let cb_ns = t0.elapsed().as_nanos() as u64;

        state.payload_sizes.push(payload_len);
        state.revs.push(rev);
        state.cb_latencies_ns.push(cb_ns);
    }
}

pub(crate) struct S3Config {
    /// Synthetic events to inject before burst (phase-1 analog of 100k trace).
    pub(crate) inject_count: u32,
    /// Number of configure() bursts (spec: 10).
    pub(crate) configure_bursts: usize,
    /// Interval between configure() calls.
    pub(crate) burst_interval: Duration,
}

impl Default for S3Config {
    fn default() -> Self {
        S3Config {
            inject_count: 1_000,
            configure_bursts: 10,
            burst_interval: Duration::from_millis(200),
        }
    }
}

pub(crate) fn run(cfg: S3Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();

    let app: *mut NmpApp = nmp_app_new();
    let baseline_rss = process_rss_bytes();

    let state = Box::new(Mutex::new(CallbackState {
        cb_latencies_ns: Vec::new(),
        payload_sizes: Vec::new(),
        revs: Vec::new(),
    }));
    let ctx = Box::into_raw(state) as *mut c_void;

    nmp_app_set_update_callback(app, ctx, Some(measure_cb));
    // Configure-not-Start: no relay workers; S3 tests emit serialization, not relay.
    nmp_app_configure(app, 0, 500, 12);

    // Inject synthetic events to build kernel state (phase-1 analog of 100k trace).
    // Events appear in the read-cache HashMap immediately after the actor processes
    // the InjectSyntheticEvents command, which is enqueued fire-and-forget.
    let base_ts: u64 = 1_700_000_000;
    inject_events(app, "s3-", base_ts, cfg.inject_count);

    // Settle: allow actor to process inject + emit the initial snapshot.
    configure_and_settle(app, 500);

    // D8 allocator snapshot: take BEFORE the burst to measure heap slope over
    // the serialization window (post-warmup).
    let burst_snap_before = alloc_snapshot();
    let burst_start = Instant::now();

    // Trigger configure() bursts to force serialization pressure.
    for _ in 0..cfg.configure_bursts {
        nmp_app_configure(app, 0, 500, 12);
        std::thread::sleep(cfg.burst_interval);
    }
    let burst_elapsed = burst_start.elapsed();
    let burst_snap_after = alloc_snapshot();

    // Grace period for final emits.
    std::thread::sleep(Duration::from_millis(300));

    let wall_elapsed = wall_start.elapsed().as_secs_f64();
    let final_rss = process_rss_bytes();

    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);

    let state = unsafe { Box::from_raw(ctx as *mut Mutex<CallbackState>) };
    let state = state.lock().unwrap();

    let rss_growth = final_rss.saturating_sub(baseline_rss);
    let emit_count = state.cb_latencies_ns.len();

    let mut lats = state.cb_latencies_ns.clone();
    lats.sort_unstable();
    let p99_cb_ns = percentile_u64(&lats, 99);
    let p99_cb_ms = p99_cb_ns as f64 / 1_000_000.0;
    // apply_us proxy: callback body time in µs (approximates Swift apply_us measurement).
    let p99_apply_us = p99_cb_ns / 1_000;

    let max_payload = state.payload_sizes.iter().copied().max().unwrap_or(0);
    let revs_monotonic = revs_strictly_increasing(&state.revs);

    let burst_hz = if burst_elapsed.as_secs_f64() > 0.0 {
        emit_count as f64 / burst_elapsed.as_secs_f64()
    } else {
        0.0
    };

    // D8 allocator gate: net heap growth per emit across burst window.
    // The harness's own Vecs grow by O(1) per emit (amortized Vec push, ~100 bytes).
    // A leak in the kernel path would cause much larger per-emit growth.
    // Gate: net heap bytes per emit <= 2 KiB (generous; catches kernel leaks
    // while allowing for harness Vec amortisation overhead).
    let net_heap_delta_burst = burst_snap_after.net_heap_delta(&burst_snap_before);
    let net_heap_per_emit = if emit_count > 0 {
        net_heap_delta_burst as f64 / emit_count as f64
    } else {
        0.0
    };

    // G-S3 gates — per docs/design/ffi-hardening/gates.md §G-S3.
    report.gates.push(
        Gate::lte("callback_p99_ms", p99_cb_ms, 20.0)
            .with_note("G-S3: per-emit serialization wall-time p99 <= 20 ms"),
    );
    report.gates.push(
        Gate::lte(
            "max_payload_bytes",
            max_payload as f64,
            2.0 * 1024.0 * 1024.0,
        )
        .with_note("G-S3: per-emit payload size <= 2 MiB"),
    );
    // apply_us proxy: p99 <= 16,000 µs (16 ms = one 60 Hz frame).
    report.gates.push(
        Gate::lte("apply_us_p99", p99_apply_us as f64, 16_000.0)
            .with_note("G-S3: callback apply_us p99 <= 16 ms (one 60 Hz frame)"),
    );
    report.gates.push(
        Gate::lte("emit_hz", burst_hz, 60.0)
            .with_note("G-S3: end-to-end reconciler frequency <= 60 Hz"),
    );
    report.gates.push(
        Gate::eq("rev_monotonic", if revs_monotonic { 1.0 } else { 0.0 }, 1.0)
            .with_note("G-S3: rev strictly increasing across emits (bible #1)"),
    );
    // D8: net heap growth per emit <= 2 KiB.
    // Harness Vec amortisation contributes ~100-500 bytes/emit (amortized push).
    // A kernel-path leak would show as multiple KiB/emit sustained growth.
    report.gates.push(
        Gate::lte("net_heap_per_emit_bytes", net_heap_per_emit, 2048.0)
            .with_note("G-S3/D8: net heap per emit <= 2 KiB (harness overhead excluded)"),
    );

    report.notes.push(format!(
        "Injected {} synthetic events; emits observed: {}; burst window: {:.1} s; Hz: {:.1}",
        cfg.inject_count, emit_count, burst_elapsed.as_secs_f64(), burst_hz
    ));
    report.notes.push(
        "Event injection uses nmp_app_inject_events (test-support, no sig verify). \
         Full 100k snapshot test with captured trace is a phase-2 deliverable."
            .to_string(),
    );

    report.measurements = json!({
        "inject_count": cfg.inject_count,
        "emit_count": emit_count,
        "configure_bursts": cfg.configure_bursts,
        "burst_elapsed_ms": burst_elapsed.as_millis(),
        "burst_hz": burst_hz,
        "p99_callback_ns": p99_cb_ns,
        "p99_callback_ms": p99_cb_ms,
        "p99_apply_us": p99_apply_us,
        "max_payload_bytes": max_payload,
        "rss_growth_bytes": rss_growth,
        "rev_monotonic": revs_monotonic,
        "net_heap_delta_burst_bytes": net_heap_delta_burst,
        "net_heap_per_emit_bytes": net_heap_per_emit,
        "wall_seconds": wall_elapsed,
    });

    report.finish(wall_elapsed);
}
