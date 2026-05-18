//! S2 — Dispatch flood (mpsc backpressure).
//!
//! Spec: docs/design/ffi-hardening/scenarios.md §S2
//! Gate: docs/design/ffi-hardening/gates.md §G-S2
//!
//! 10,000 dispatches/sec from N=4 caller threads × 60 s.
//! Mix: 30% open_author, 30% close_author, 20% claim_profile, 20% release_profile.
//!
//! D2 (<=60 Hz reactivity bound): actor mpsc backlog never exceeds 10,000.
//! Bible #3 (fire-and-forget): every send call returns within p99 <= 1 ms.

use crate::ffi::{
    nmp_app_claim_profile, nmp_app_close_author, nmp_app_free, nmp_app_new, nmp_app_open_author,
    nmp_app_release_profile, nmp_app_set_update_callback, nmp_app_start, process_rss_bytes,
    test_pubkeys, NmpApp,
};
use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use serde_json::json;
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

static CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);

extern "C" fn sink_cb(_ctx: *mut c_void, _payload: *const c_char) {
    CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub(crate) struct S2Config {
    /// Wall duration (spec: 60 s).
    pub(crate) duration: Duration,
    /// Caller thread count (spec: 4).
    pub(crate) threads: usize,
    /// Total dispatches per second across all threads (spec: 10,000).
    pub(crate) dispatches_per_sec: u64,
    /// Pubkey pool size (spec: 50).
    pub(crate) pool_size: usize,
}

impl Default for S2Config {
    fn default() -> Self {
        S2Config {
            duration: Duration::from_secs(30),
            threads: 4,
            dispatches_per_sec: 10_000,
            pool_size: 50,
        }
    }
}

pub(crate) fn run(cfg: S2Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();

    let app: *mut NmpApp = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(sink_cb));
    nmp_app_start(app, 0, 80, 4);

    let baseline_rss = process_rss_bytes();

    // Convert raw pointer to usize for Send-safe sharing across threads.
    let app_usize = app as usize;
    let pubkeys_arc: Arc<Vec<CString>> = Arc::new(test_pubkeys(cfg.pool_size));

    // Per-thread rate: total / threads.
    let per_thread_dps = cfg.dispatches_per_sec / cfg.threads as u64;
    let interval_ns = 1_000_000_000u64 / per_thread_dps.max(1);
    let duration = cfg.duration;
    let threads = cfg.threads;

    // Barrier so all threads start dispatching simultaneously.
    let barrier = Arc::new(Barrier::new(cfg.threads));

    // Collect per-thread latency samples.
    let latency_collector: Arc<std::sync::Mutex<Vec<u64>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let dispatch_counter: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..cfg.threads)
        .map(|thread_idx| {
            let barrier = Arc::clone(&barrier);
            let pubkeys = Arc::clone(&pubkeys_arc);
            let latencies = Arc::clone(&latency_collector);
            let counter = Arc::clone(&dispatch_counter);

            std::thread::spawn(move || {
                let app_ptr = app_usize as *mut NmpApp;
                barrier.wait();
                let thread_start = Instant::now();
                let mut local_latencies: Vec<u64> = Vec::with_capacity(1024);
                let mut seq: u64 = thread_idx as u64;
                let mut next_tick = Instant::now();

                while thread_start.elapsed() < duration {
                    let pk_idx = seq as usize % pubkeys.len();
                    let pk = &pubkeys[pk_idx];
                    // Mix per spec: 30/30/20/20.
                    let dispatch_kind = seq % 10;

                    let t0 = Instant::now();
                    match dispatch_kind {
                        0..=2 => nmp_app_open_author(app_ptr, pk.as_ptr()),
                        3..=5 => nmp_app_close_author(app_ptr, pk.as_ptr()),
                        6..=7 => {
                            let consumer =
                                CString::new(format!("t{thread_idx}-{seq}")).expect("no nuls");
                            nmp_app_claim_profile(app_ptr, pk.as_ptr(), consumer.as_ptr());
                        }
                        _ => {
                            let consumer =
                                CString::new(format!("t{thread_idx}-{seq}")).expect("no nuls");
                            nmp_app_release_profile(app_ptr, pk.as_ptr(), consumer.as_ptr());
                        }
                    }
                    let elapsed_ns = t0.elapsed().as_nanos() as u64;
                    local_latencies.push(elapsed_ns);
                    counter.fetch_add(1, Ordering::Relaxed);
                    seq += threads as u64;

                    next_tick += Duration::from_nanos(interval_ns);
                    if let Some(sleep) = next_tick.checked_duration_since(Instant::now()) {
                        std::thread::sleep(sleep);
                    }
                }

                latencies.lock().unwrap().extend(local_latencies);
            })
        })
        .collect();

    for handle in handles {
        let _ = handle.join();
    }

    let wall_elapsed = wall_start.elapsed().as_secs_f64();
    let final_rss = process_rss_bytes();
    let rss_growth_bytes = final_rss.saturating_sub(baseline_rss);
    let total_dispatches = dispatch_counter.load(Ordering::Relaxed);

    // Compute latency percentiles.
    let mut latencies = latency_collector.lock().unwrap().clone();
    latencies.sort_unstable();
    let p50_ns = percentile(&latencies, 50);
    let p99_ns = percentile(&latencies, 99);

    // G-S2 numeric gates.
    let p99_ms = p99_ns as f64 / 1_000_000.0;
    let p50_ms = p50_ns as f64 / 1_000_000.0;
    let nominal = cfg.dispatches_per_sec * cfg.duration.as_secs();

    report.gates.push(
        Gate::gte("dispatches_submitted", total_dispatches as f64, 600_000.0).with_note(
            "G-S2: dispatches_submitted >= 600,000 over 60 s full (>=300k fast)",
        ),
    );
    report.gates.push(
        Gate::lte("send_latency_p99_ms", p99_ms, 1.0)
            .with_note("G-S2/bible#3: Swift->Rust send latency p99 <= 1 ms"),
    );
    report.gates.push(
        Gate::lte("send_latency_p50_ms", p50_ms, 0.1)
            .with_note("G-S2: Swift->Rust send latency p50 <= 100 us"),
    );
    report.gates.push(
        Gate::lte(
            "rss_growth_bytes",
            rss_growth_bytes as f64,
            20.0 * 1024.0 * 1024.0,
        )
        .with_note("G-S2: RSS growth <= 20 MiB over 60 s"),
    );

    report.notes.push(format!(
        "Nominal dispatches: {}; actual: {}; p50={:.3}ms p99={:.3}ms",
        nominal, total_dispatches, p50_ms, p99_ms,
    ));
    report.notes.push(
        "Actor mpsc backlog depth: not directly observable from caller thread; \
         RSS growth is the proxy gate (bounded channel growth = bounded RSS)."
            .to_string(),
    );

    report.measurements = json!({
        "total_dispatches": total_dispatches,
        "nominal_dispatches": nominal,
        "threads": cfg.threads,
        "dispatches_per_sec": cfg.dispatches_per_sec,
        "p50_ns": p50_ns,
        "p99_ns": p99_ns,
        "p50_ms": p50_ms,
        "p99_ms": p99_ms,
        "rss_growth_bytes": rss_growth_bytes,
        "callback_count": CALLBACK_COUNT.load(Ordering::Relaxed),
        "wall_seconds": wall_elapsed,
        "latency_samples": latencies.len(),
    });

    // Teardown.
    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);

    report.finish(wall_elapsed);
}

fn percentile(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) * pct) / 100;
    sorted[idx]
}
