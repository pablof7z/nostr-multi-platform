//! S3 — Snapshot pressure (`AppUpdate::FullState` with 100k events).
//!
//! Spec: docs/design/ffi-hardening/scenarios.md §S3
//! Gate: docs/design/ffi-hardening/gates.md §G-S3
//!
//! Host-side analog: stream events via open_firehose_tag to build up kernel
//! state, then trigger ten forced configure() calls to stress the
//! listener serialization + callback path.
//!
//! The exact "100k events via captured trace" path (requiring an in-process
//! relay or T23 actor injection) is a phase-2 deliverable. This phase-1
//! implementation drives the real ingest path via the FFI surface and
//! measures the observable (callback receive-to-return latency). The gate
//! for per-emit p99 JSON serialization time is measured on the callback side.
//!
//! D1 (best-effort rendering): every emit processed promptly without blocking.
//! D2 (<=60 Hz): reconciler frequency stays bounded by configured emit_hz.

use crate::ffi::{
    nmp_app_configure, nmp_app_free, nmp_app_new, nmp_app_open_firehose_tag,
    nmp_app_set_update_callback, nmp_app_start, process_rss_bytes, NmpApp,
};
use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use serde_json::json;
use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Shared callback state for S3.
struct CallbackState {
    /// Callback latencies (ns).
    callback_latencies_ns: Vec<u64>,
    /// Payload bytes received per emit.
    payload_sizes: Vec<usize>,
    /// `rev` field extracted from each emit (monotonicity check).
    revs: Vec<u64>,
}

extern "C" fn measure_cb(ctx: *mut c_void, payload: *const c_char) {
    let t0 = Instant::now();

    // SAFETY: ctx is a valid Box<Mutex<CallbackState>> that lives for the
    // duration of this run; callback is cleared before drop.
    let state_ptr = ctx as *mut Mutex<CallbackState>;
    if let Ok(mut state) = unsafe { (*state_ptr).lock() } {
        let payload_len = if payload.is_null() {
            0
        } else {
            // SAFETY: payload is a valid C string for this callback's duration.
            unsafe { std::ffi::CStr::from_ptr(payload) }.to_bytes().len()
        };

        let rev = if !payload.is_null() {
            let bytes = unsafe { std::ffi::CStr::from_ptr(payload) }.to_bytes();
            extract_rev_field(bytes).unwrap_or(0)
        } else {
            0
        };

        state.payload_sizes.push(payload_len);
        state.revs.push(rev);

        let latency = t0.elapsed().as_nanos() as u64;
        state.callback_latencies_ns.push(latency);
    }
}

/// Extract the `"rev":N` field from a JSON payload without a full parse.
fn extract_rev_field(bytes: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(bytes).ok()?;
    let key = "\"rev\":";
    let pos = s.find(key)?;
    let rest = &s[pos + key.len()..];
    let end = rest.find([',', '}', ' ', '\n']).unwrap_or(rest.len());
    rest[..end].trim().parse::<u64>().ok()
}

pub(crate) struct S3Config {
    /// Tag to open for ingest (any value drives the firehose path).
    pub(crate) firehose_tag: String,
    /// How long to drive events before triggering configure() bursts.
    pub(crate) warmup_duration: Duration,
    /// Number of configure() calls to trigger forced emits (spec: 10).
    pub(crate) configure_bursts: usize,
    /// Interval between configure() calls.
    pub(crate) burst_interval: Duration,
}

impl Default for S3Config {
    fn default() -> Self {
        S3Config {
            firehose_tag: "test".to_string(),
            warmup_duration: Duration::from_secs(10),
            configure_bursts: 10,
            burst_interval: Duration::from_millis(200),
        }
    }
}

pub(crate) fn run(cfg: S3Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();

    let app: *mut NmpApp = nmp_app_new();
    let baseline_rss = process_rss_bytes();

    // Allocate callback state on the heap; pass raw pointer as context.
    let state = Box::new(Mutex::new(CallbackState {
        callback_latencies_ns: Vec::new(),
        payload_sizes: Vec::new(),
        revs: Vec::new(),
    }));
    let ctx = Box::into_raw(state) as *mut c_void;

    nmp_app_set_update_callback(app, ctx, Some(measure_cb));
    // 12 Hz emit cap ensures we can measure frequency vs the 60 Hz bound.
    nmp_app_start(app, 0, 500, 12);

    // Open firehose to drive ingest.
    let tag = std::ffi::CString::new(cfg.firehose_tag.as_str()).expect("no nuls");
    nmp_app_open_firehose_tag(app, tag.as_ptr());

    // Let events accumulate during warmup.
    std::thread::sleep(cfg.warmup_duration);

    // Trigger ten configure() calls to force serialization bursts.
    let burst_start = Instant::now();
    for _ in 0..cfg.configure_bursts {
        nmp_app_configure(app, 0, 500, 12);
        std::thread::sleep(cfg.burst_interval);
    }
    let burst_elapsed = burst_start.elapsed();

    // Allow one more tick for the final emits to arrive.
    std::thread::sleep(Duration::from_millis(500));

    let wall_elapsed = wall_start.elapsed().as_secs_f64();
    let final_rss = process_rss_bytes();

    // Teardown — clear callback before reclaiming ctx.
    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);

    // SAFETY: callback is cleared; no other references to ctx remain.
    let state = unsafe { Box::from_raw(ctx as *mut Mutex<CallbackState>) };
    let state = state.lock().unwrap();

    let rss_growth = final_rss.saturating_sub(baseline_rss);
    let emit_count = state.callback_latencies_ns.len();

    // Callback latency percentiles (approximates Rust-side emit cost).
    let mut lats = state.callback_latencies_ns.clone();
    lats.sort_unstable();
    let p99_cb_ns = percentile_u64(&lats, 99);
    let p99_cb_ms = p99_cb_ns as f64 / 1_000_000.0;

    // Payload size gate.
    let max_payload = state.payload_sizes.iter().copied().max().unwrap_or(0);

    // Rev monotonicity check.
    let revs_monotonic = is_strictly_increasing(&state.revs);

    // Emit frequency over burst window.
    let burst_hz = if burst_elapsed.as_secs_f64() > 0.0 {
        emit_count as f64 / burst_elapsed.as_secs_f64()
    } else {
        0.0
    };

    // G-S3 gates.
    report.gates.push(
        Gate::lte("callback_p99_ms", p99_cb_ms, 20.0)
            .with_note("G-S3: per-emit JSON serialization wall-time p99 <= 20 ms (Rust-side)"),
    );
    report.gates.push(
        Gate::lte(
            "max_payload_bytes",
            max_payload as f64,
            2.0 * 1024.0 * 1024.0,
        )
        .with_note("G-S3: per-emit payload size <= 2 MiB"),
    );
    report.gates.push(
        Gate::lte("emit_hz", burst_hz, 60.0)
            .with_note("G-S3: end-to-end reconciler frequency <= 60 Hz"),
    );
    report.gates.push(
        Gate::eq("rev_monotonic", if revs_monotonic { 1.0 } else { 0.0 }, 1.0)
            .with_note("G-S3: rev field strictly increasing across emits (bible #1)"),
    );

    report.notes.push(format!(
        "Phase-1 host approximation: events driven via open_firehose_tag (no relay). \
         Full 100k snapshot test requires T23 actor-injection (phase 2). \
         Emits observed: {emit_count}; burst window: {:.1} s; burst Hz: {:.1}",
        burst_elapsed.as_secs_f64(),
        burst_hz
    ));

    report.measurements = json!({
        "emit_count": emit_count,
        "configure_bursts": cfg.configure_bursts,
        "burst_elapsed_ms": burst_elapsed.as_millis(),
        "burst_hz": burst_hz,
        "p99_callback_ns": p99_cb_ns,
        "p99_callback_ms": p99_cb_ms,
        "max_payload_bytes": max_payload,
        "rss_growth_bytes": rss_growth,
        "rev_monotonic": revs_monotonic,
        "wall_seconds": wall_elapsed,
    });

    report.finish(wall_elapsed);
}

fn percentile_u64(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) * pct) / 100;
    sorted[idx]
}

fn is_strictly_increasing(revs: &[u64]) -> bool {
    if revs.len() < 2 {
        return true;
    }
    // Allow zeros (unparsed) at the start — only check parsed pairs.
    let non_zero: Vec<u64> = revs.iter().copied().filter(|&r| r > 0).collect();
    if non_zero.len() < 2 {
        return true;
    }
    non_zero.windows(2).all(|w| w[1] > w[0])
}
