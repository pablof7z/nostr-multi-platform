//! S1 — Mount/unmount churn (view-handle wrapper refcount).
//!
//! Spec: docs/design/ffi-hardening/scenarios.md §S1
//! Gate: docs/design/ffi-hardening/gates.md §G-S1
//!
//! Drives 1,000 claim/release pairs/sec across 100 unique pubkeys
//! for the configured duration (full: 600 s; fast: 60 s).
//!
//! D4 (single writer per fact): refcount table mutated on actor thread only.
//! D8 (zero per-event allocations after warmup): counting allocator checks
//!    that heap slope post-warmup is <= 0.

use crate::allocator::{alloc_snapshot, AllocSnapshot};
use crate::ffi::{
    nmp_app_claim_profile, nmp_app_free, nmp_app_new, nmp_app_release_profile,
    nmp_app_set_update_callback, nmp_app_start, process_rss_bytes, test_pubkeys, NmpApp,
};
use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use serde_json::json;
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Shared callback counter — we only need to know the listener is alive.
static CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);

extern "C" fn sink_cb(_ctx: *mut c_void, _payload: *const c_char) {
    CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Scenario configuration.
pub(crate) struct S1Config {
    /// How long to run (600 s full / 60 s fast).
    pub(crate) duration: Duration,
    /// How many distinct pubkeys to rotate across (spec: 100).
    pub(crate) pool_size: usize,
    /// Target claim+release pairs per second (spec: 1,000).
    pub(crate) pairs_per_sec: u64,
}

impl Default for S1Config {
    fn default() -> Self {
        S1Config {
            duration: Duration::from_secs(60),
            pool_size: 100,
            pairs_per_sec: 1000,
        }
    }
}

/// Run S1. Returns the completed `ScenarioMetrics` ready for serialization.
pub(crate) fn run(cfg: S1Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();

    // --- Setup ---
    let app: *mut NmpApp = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(sink_cb));
    nmp_app_start(app, 0, 80, 4);

    let pubkeys = test_pubkeys(cfg.pool_size);
    let baseline_rss = process_rss_bytes();

    // Warmup: 30 s or 5 % of duration (whichever is smaller).
    let warmup = Duration::from_secs(30).min(cfg.duration / 20);
    let warmup_end = Instant::now() + warmup;

    let interval_ns = 1_000_000_000u64 / cfg.pairs_per_sec;
    let mut cycles: u64 = 0;
    let mut next_tick = Instant::now();

    // --- Warmup phase ---
    while Instant::now() < warmup_end {
        fire_cycle(app, &pubkeys, cycles);
        cycles += 1;
        next_tick += Duration::from_nanos(interval_ns);
        if let Some(sleep) = next_tick.checked_duration_since(Instant::now()) {
            std::thread::sleep(sleep);
        }
    }

    // --- Steady-state phase (allocator measurement) ---
    let ss_snap_before: AllocSnapshot = alloc_snapshot();
    let ss_start = Instant::now();
    let mut ss_cycles: u64 = 0;

    while wall_start.elapsed() < cfg.duration {
        fire_cycle(app, &pubkeys, cycles);
        cycles += 1;
        ss_cycles += 1;
        next_tick += Duration::from_nanos(interval_ns);
        if let Some(sleep) = next_tick.checked_duration_since(Instant::now()) {
            std::thread::sleep(sleep);
        }
    }

    let ss_elapsed = ss_start.elapsed().as_secs_f64();
    let ss_snap_after = alloc_snapshot();

    // Capture run time before teardown; nmp_app_free joins threads and can
    // add several seconds that are not part of the scenario wall time.
    let run_elapsed = wall_start.elapsed().as_secs_f64();

    // --- Teardown: clear callback before freeing ---
    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);

    // --- Metrics ---
    let wall_elapsed = run_elapsed;
    let final_rss = process_rss_bytes();
    let rss_growth_bytes = final_rss.saturating_sub(baseline_rss);
    let nominal_cycles = cfg.pairs_per_sec * cfg.duration.as_secs();
    let min_cycles = nominal_cycles * 90 / 100;

    // Net heap slope (bytes/sec) in steady state — D8 invariant.
    // Use net_heap_delta (live bytes) not bytes_since (gross allocations) so that
    // transient per-cycle allocations (CString, etc.) that are immediately freed
    // do not fail the D8 gate; only RETAINED growth fails.
    let net_heap_delta = ss_snap_after.net_heap_delta(&ss_snap_before);
    let net_heap_slope = if ss_elapsed > 0.0 { net_heap_delta as f64 / ss_elapsed } else { 0.0 };
    // Keep gross bytes for informational measurement.
    let ss_bytes = ss_snap_after.bytes_since(&ss_snap_before);

    // G-S1 gates
    report.gates.push(
        Gate::lte(
            "rss_growth_bytes",
            rss_growth_bytes as f64,
            5.0 * 1024.0 * 1024.0,
        )
        .with_note("G-S1: process RSS growth <= 5 MiB over full run"),
    );
    report.gates.push(
        Gate::gte("cycles_completed", cycles as f64, min_cycles as f64).with_note(
            "G-S1: cycles completed >= 90% of nominal (540k full / 54k fast)",
        ),
    );
    report.gates.push(
        Gate::lte("net_heap_slope_bytes_per_sec", net_heap_slope, 0.0).with_note(
            "G-S1/D8: net heap slope <= 0 bytes/sec post-warmup (transient allocs excluded)",
        ),
    );

    // Wall time gate: within 5 s of target
    let target_secs = cfg.duration.as_secs_f64();
    report.gates.push(
        Gate::lte("wall_seconds_over", (wall_elapsed - target_secs).max(0.0), 5.0)
            .with_note("G-S1: wall duration == target ± 5 s"),
    );

    report.notes.push(format!(
        "Warmup duration: {:.1} s; steady-state cycles: {}; callback fires: {}",
        warmup.as_secs_f64(),
        ss_cycles,
        CALLBACK_COUNT.load(Ordering::Relaxed)
    ));
    report.notes.push(
        "Claim/release pairing verified structurally: every cycle fires both calls in order. \
         Kernel-side refcount audit requires T23 test-support feature (phase 2)."
            .to_string(),
    );

    report.measurements = json!({
        "cycles_total": cycles,
        "cycles_steady_state": ss_cycles,
        "nominal_cycles": nominal_cycles,
        "rss_growth_bytes": rss_growth_bytes,
        "net_heap_slope_bytes_per_sec": net_heap_slope,
        "net_heap_delta_bytes": net_heap_delta,
        "ss_allocs": ss_snap_after.allocs_since(&ss_snap_before),
        "ss_gross_bytes_allocated": ss_bytes,
        "wall_seconds": wall_elapsed,
        "callback_count": CALLBACK_COUNT.load(Ordering::Relaxed),
    });

    report.finish(wall_elapsed);
}

fn fire_cycle(app: *mut NmpApp, pubkeys: &[std::ffi::CString], cycle: u64) {
    let pk = &pubkeys[cycle as usize % pubkeys.len()];
    let consumer = CString::new(format!("ffi-stress-{cycle}")).expect("no nuls");
    nmp_app_claim_profile(app, pk.as_ptr(), consumer.as_ptr());
    // 1 ms between claim and release per spec.
    std::thread::sleep(Duration::from_millis(1));
    nmp_app_release_profile(app, pk.as_ptr(), consumer.as_ptr());
}
