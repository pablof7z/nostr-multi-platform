//! S6 — Single-projection churn (ADR-0055 Rung 0 measurement scenario).
//!
//! **Purpose:** quantify the O(total-state) waste that ADR-0055 will fix.
//!
//! The scenario drives a workload where **only one projection family changes
//! per emit cycle** while the remaining built-in projections are static.
//! Claiming a profile dirties a small CLUSTER of related projections
//! (`claimed_profiles` + `resolved_profiles`, plus any derived view), so the
//! change count per tick is ~3 of the ~16 typed projections, not literally 1 —
//! this is why the observed change_ratio is ~19%, not ~6%. It then measures,
//! per emit:
//!   - how many typed projections were re-serialized (expected: all of them)
//!   - how many actually changed vs the previous tick (the claim cluster)
//!   - the wasted-bytes ratio: `(serialized - changed) / serialized`
//!
//! This is the empirical anchor for ADR-0055 D1–D8: it proves the O(N) waste
//! exists before any rev-gating mechanism is built.
//!
//! **Workload:** after injecting a small event batch to settle the kernel, we
//! cycle a single profile claim (claim → release → claim → …) through
//! `nmp_app_claim_profile` / `nmp_app_release_profile`. Each cycle dirties
//! the `claimed_profiles` + `resolved_profiles` cluster while all others
//! remain unchanged. Between cycles we call `nmp_app_configure` to trigger
//! one emit tick and read the churn counters.
//!
//! **Gates (informational, not PASS/FAIL):** there are no hard gates in this
//! scenario — it is a measurement scenario, not a correctness scenario.
//! The waste_ratio metric is reported as a measurement and printed in the PR
//! body. A waste_ratio ≥ 0.80 (≥80% of bytes were for unchanged projections)
//! is expected pre-ADR-0055 with ~16 built-ins and one claim cluster changing
//! per cycle.
//!
//! D0: uses `nmp_app_inject_signed_events` and `nmp_app_claim_profile` /
//! `nmp_app_release_profile` — both are cfg-gated test paths.
//! D8: no polling; cycles are driven by explicit configure() calls, not sleep.

use crate::common::{configure_and_settle, inject_signed_events, percentile_u64};
use crate::ffi::{
    nmp_app_claim_profile, nmp_app_configure, nmp_app_free, nmp_app_new,
    nmp_app_release_profile, nmp_app_set_update_callback, test_pubkeys, NmpApp,
};
use crate::report::ScenarioMetrics;
use serde_json::json;
use std::ffi::{c_void, CString};
use std::sync::Mutex;
use std::time::{Duration, Instant};

// ADR-0055 Rung 0: churn counter reader from nmp-ffi (test-support feature).
use nmp_ffi::nmp_app_read_projection_churn_stats;

/// Per-emit metrics captured in the callback.
struct CallbackState {
    /// Total serialized payload sizes per emit (bytes).
    payload_sizes: Vec<usize>,
}

extern "C" fn measure_cb(ctx: *mut c_void, payload: *const u8, payload_len: usize) {
    let state_ptr = ctx as *mut Mutex<CallbackState>;
    if let Ok(mut state) = unsafe { (*state_ptr).lock() } {
        let len = if !payload.is_null() && payload_len > 0 {
            payload_len
        } else {
            0
        };
        state.payload_sizes.push(len);
    }
}

pub(crate) struct S6Config {
    /// Number of events to inject before the measurement window.
    /// Small enough to settle quickly; enough to give the kernel real data.
    pub(crate) seed_events: u32,
    /// Number of claim-release cycles to drive during the measurement window.
    pub(crate) churn_cycles: usize,
    /// Settle wait after seed injection (ms).
    pub(crate) settle_ms: u64,
}

impl Default for S6Config {
    fn default() -> Self {
        S6Config {
            seed_events: 200,
            churn_cycles: 50,
            settle_ms: 3_000,
        }
    }
}

pub(crate) fn run(cfg: S6Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();

    let app: *mut NmpApp = nmp_app_new();

    let state = Box::new(Mutex::new(CallbackState {
        payload_sizes: Vec::new(),
    }));
    let ctx = Box::into_raw(state) as *mut c_void;

    nmp_app_set_update_callback(app, ctx, Some(measure_cb));
    // Configure without relay workers — this scenario exercises serialization,
    // not relay connectivity.
    nmp_app_configure(app, 0, 500, 12);

    // ── Seed phase ───────────────────────────────────────────────────────────
    // Inject a modest event batch so the kernel has real profile data,
    // claimed_profiles, resolved_profiles, etc. built up.
    let base_ts: u64 = 1_700_000_000;
    inject_signed_events(app, base_ts, cfg.seed_events);
    configure_and_settle(app, cfg.settle_ms);

    // Generate a fixed pubkey + consumer_id for the churn target.
    let pubkeys = test_pubkeys(1);
    let churn_pubkey = &pubkeys[0];
    let consumer_id = CString::new("s6-churn-consumer").expect("valid CString");

    // ── Baseline churn counter snapshot ──────────────────────────────────────
    // Read BEFORE the measurement window so we isolate only the churn cycles.
    let mut baseline_serialized: u64 = 0;
    let mut baseline_changed: u64 = 0;
    nmp_app_read_projection_churn_stats(&mut baseline_serialized, &mut baseline_changed);

    let window_start = Instant::now();

    // ── Measurement window: cycle one projection per emit ────────────────────
    //
    // Each iteration:
    //   1. claim_profile → dirties `claimed_profiles` + `resolved_profiles`
    //   2. configure()   → triggers one emit tick
    //   3. release_profile → undirties those projections
    //   4. configure()   → triggers another emit tick
    //
    // So every other tick has exactly ONE changed projection family;
    // the other ~16 built-ins are unchanged. This is the worst-case O(N)
    // pattern the ADR fixes.
    //
    // D8: no sleep loops — each configure() drives one synchronous emit
    // tick; the 200 ms sleep is a wall-clock gate to let the actor process.
    for _ in 0..cfg.churn_cycles {
        // Dirty one projection: claim the churn pubkey (force=0 = normal claim).
        nmp_app_claim_profile(app, churn_pubkey.as_ptr(), consumer_id.as_ptr(), 0);
        // Wait for the actor to emit; 200 ms > one 4Hz tick (250 ms interval).
        std::thread::sleep(Duration::from_millis(200));
        nmp_app_configure(app, 0, 500, 12);
        std::thread::sleep(Duration::from_millis(50));

        // Un-dirty: release the claim.
        nmp_app_release_profile(app, churn_pubkey.as_ptr(), consumer_id.as_ptr());
        std::thread::sleep(Duration::from_millis(200));
        nmp_app_configure(app, 0, 500, 12);
        std::thread::sleep(Duration::from_millis(50));
    }

    let window_elapsed = window_start.elapsed();

    // ── End churn counter snapshot ────────────────────────────────────────────
    let mut end_serialized: u64 = 0;
    let mut end_changed: u64 = 0;
    nmp_app_read_projection_churn_stats(&mut end_serialized, &mut end_changed);

    let window_serialized = end_serialized.saturating_sub(baseline_serialized);
    let window_changed = end_changed.saturating_sub(baseline_changed);
    let window_wasted = window_serialized.saturating_sub(window_changed);

    let waste_ratio = if window_serialized > 0 {
        window_wasted as f64 / window_serialized as f64
    } else {
        0.0
    };
    let change_ratio = if window_serialized > 0 {
        window_changed as f64 / window_serialized as f64
    } else {
        0.0
    };

    let wall_elapsed = wall_start.elapsed().as_secs_f64();

    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);

    let state = unsafe { Box::from_raw(ctx as *mut Mutex<CallbackState>) };
    let state = state.lock().unwrap();

    let emit_count = state.payload_sizes.len();
    let mut sizes = state.payload_sizes.clone();
    sizes.sort_unstable();
    let p50_payload = percentile_u64(&sizes.iter().map(|&s| s as u64).collect::<Vec<_>>(), 50);
    let p99_payload = percentile_u64(&sizes.iter().map(|&s| s as u64).collect::<Vec<_>>(), 99);

    // ── Report ────────────────────────────────────────────────────────────────
    // No PASS/FAIL gates — this is a measurement scenario. The waste_ratio and
    // change_ratio are the empirical anchor numbers for the PR body.
    report.notes.push(format!(
        "S6 single-projection-churn: {} emit cycles, {} churn iterations",
        emit_count, cfg.churn_cycles
    ));
    report.notes.push(format!(
        "Projection churn window: serialized={} changed={} wasted={} \
         waste_ratio={:.1}% change_ratio={:.1}%",
        window_serialized,
        window_changed,
        window_wasted,
        waste_ratio * 100.0,
        change_ratio * 100.0,
    ));
    report.notes.push(format!(
        "Payload: p50={}B p99={}B over {} emits in {:.1}s",
        p50_payload,
        p99_payload,
        emit_count,
        window_elapsed.as_secs_f64()
    ));
    report.notes.push(
        "ADR-0055 Rung 1+3 target: change_ratio approaches 1/N_projections \
         (≈5-6% for 18 built-ins); waste_ratio drops from ~95% to ~0% at the floor."
            .to_string(),
    );

    report.measurements = json!({
        "seed_events": cfg.seed_events,
        "churn_cycles": cfg.churn_cycles,
        "window_elapsed_ms": window_elapsed.as_millis(),
        "emit_count_total": emit_count,
        "window_projections_serialized": window_serialized,
        "window_projections_changed": window_changed,
        "window_projections_wasted": window_wasted,
        "waste_ratio": waste_ratio,
        "change_ratio": change_ratio,
        "p50_payload_bytes": p50_payload,
        "p99_payload_bytes": p99_payload,
        "wall_seconds": wall_elapsed,
    });

    report.finish(wall_elapsed);
    // Mark as passed (measurement-only; no hard gates).
    report.passed = true;
}
