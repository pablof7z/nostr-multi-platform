//! S6 — Single-projection churn (ADR-0055 Rung 3 capstone measurement).
//!
//! **Purpose:** empirical PASS/FAIL gate proving that Rung 3's producer-side
//! omission of `Unchanged` projections suppresses Tier-2 wire rows on the
//! single-projection (refs.profile) churn workload, with zero data loss
//! (ADR-0055 §4 R3-S5 + §9).
//!
//! **Honest measured result (reproducible, 5× by the methodology reviewer):**
//! - Tier-2 row suppression: **1600 → 500 rows over the window (68.8%)** — every
//!   `Unchanged` Tier-2 row is omitted; zero unchanged-Tier-2 leaks.
//! - Frame bytes: **9640B → 7928B p50 (~18% reduction)**, zero data loss.
//! - The remaining frame bytes are dominated by the two Tier-1 (feed-class) keys
//!   `refs.event.envelopes` + `nip46_onboarding`, which are always-Changed by
//!   D3-7 (no manifest entry) and are gated in a LATER rung — NOT this one.
//!
//! Note on the hash-based `waste_ratio`: Phase B still reads ~40% by that
//! (Rung-0) metric, but that residue is **entirely the two out-of-scope Tier-1
//! keys**, not unsuppressed Tier-2 rows. That is why the capstone gate is
//! `row_suppression_ratio`, not `waste_ratio` (see [`crate::s6_gates`]).
//!
//! **Two-phase measurement:**
//! Phase A (baseline, incremental OFF): all projections serialized every tick.
//! Phase B (incremental ON): a second `NmpApp` with `nmp_app_declare_incremental_apply`
//! called before the window — only `Changed`/`Cleared` rows emitted; `Unchanged`
//! rows omitted.
//!
//! **Byte-identity oracle (correctness proof, fail-closed):** [`crate::s6_oracle`]
//! replays the incremental stream through a Rust stand-in of the ProjectionCache
//! merge (Changed→overwrite, Cleared→drop, absent→retain) and asserts the
//! **end-state** reconstruction is byte-identical to Phase A's final full-frame
//! set. It is fail-closed: a dropped Tier-2 row hard-fails the gate; only the two
//! known-nondeterministic Tier-1 keys above may be absent.
//!
//! **Hard PASS/FAIL gates ([`crate::s6_gates`], ADR-0055 §9 / R3-S5 mandate):**
//! - `row_suppression_ratio ≥ 0.50` (Tier-2 rows suppressed by omit-Unchanged)
//! - `p50_frame_bytes_incremental < p50_frame_bytes_baseline`
//! - `serialize_us` p50 incremental ≤ baseline × 1.20 (no encode-time regression)
//! - Byte-identity oracle PASS (end-state reconstruction == full-frame, fail-closed)
//!
//! **Metric honesty (ADR-0055 §3 D3-7 / codex Q4):** the suppression measurement
//! covers Tier-2 single-projection (refs.profile) churn only. Tier-1 (feed-class) projections
//! stay always-Changed in Rung 3 — gating them is a later rung. The report
//! notes line makes this explicit.
//!
//! D0: uses `nmp_app_inject_signed_events` and `nmp_app_resolve_ref` /
//! `nmp_app_release_ref` (profile namespace) — both are cfg-gated test paths.
//! D8: no polling; cycles are driven by `configure_and_await_frame` (event-driven
//! waits via `FrameProbe`) — no busy-wait loops.

use crate::common::{configure_and_await_frame, inject_signed_events, percentile_u64};
use crate::ffi::{
    nmp_app_configure, nmp_app_free, nmp_app_new, nmp_app_release_ref, nmp_app_resolve_ref,
    nmp_app_set_update_callback, test_pubkeys, NmpApp,
};
use crate::report::ScenarioMetrics;
use crate::s6_gates::{apply as apply_gates, PhaseMetrics, S6Outcome};
use crate::s6_oracle::{run_byte_identity_oracle, FrameRecord};
use nmp_core::decode_snapshot_typed_projections;
use nmp_ffi::{nmp_app_declare_incremental_apply, nmp_app_read_projection_churn_stats};
use nmp_testing::harness_probe::{FrameProbe, ProbeSignal};
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::Instant;

// ── Per-tick callback capture ─────────────────────────────────────────────────

/// Phase A capture: frame records only (no raw bytes needed — Phase A is the
/// full-frame reference; the oracle reconstructs from Phase B's bytes).
struct CallbackState {
    signal: ProbeSignal,
    records: Vec<FrameRecord>,
}

/// Phase B capture: frame records **plus** the raw FlatBuffers bytes per tick,
/// which the byte-identity oracle replays through the ProjectionCache stand-in.
struct ByteCapture {
    signal: ProbeSignal,
    records: Vec<FrameRecord>,
    raw_frames: Vec<Vec<u8>>,
}

/// Decode one delivered frame into a [`FrameRecord`] (shared by both phases).
fn decode_frame_record(bytes: &[u8]) -> FrameRecord {
    let serialize_us = nmp_core::decode_snapshot_envelope(bytes)
        .map(|env| env.serialize_us)
        .unwrap_or(0);
    let projection_payloads = decode_snapshot_typed_projections(bytes)
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.key, p.payload))
        .collect();
    FrameRecord {
        frame_bytes: bytes.len(),
        serialize_us,
        projection_payloads,
    }
}

extern "C" fn measure_cb(ctx: *mut c_void, payload: *const u8, payload_len: usize) {
    let state_ptr = ctx as *mut Mutex<CallbackState>;
    if let Ok(mut state) = unsafe { (*state_ptr).lock() } {
        if payload.is_null() || payload_len == 0 {
            return;
        }
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(payload, payload_len) };
        state.records.push(decode_frame_record(bytes));
        state.signal.notify();
    }
}

extern "C" fn measure_cb_with_bytes(ctx: *mut c_void, payload: *const u8, payload_len: usize) {
    let state_ptr = ctx as *mut Mutex<ByteCapture>;
    if let Ok(mut state) = unsafe { (*state_ptr).lock() } {
        if payload.is_null() || payload_len == 0 {
            return;
        }
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(payload, payload_len) };
        state.records.push(decode_frame_record(bytes));
        state.raw_frames.push(bytes.to_vec());
        state.signal.notify();
    }
}

fn callback_record_count(ctx: *mut c_void) -> usize {
    let state_ptr = ctx as *mut Mutex<CallbackState>;
    unsafe { (*state_ptr).lock() }
        .map(|state| state.records.len())
        .unwrap_or(0)
}

fn byte_capture_record_count(ctx: *mut c_void) -> usize {
    let state_ptr = ctx as *mut Mutex<ByteCapture>;
    unsafe { (*state_ptr).lock() }
        .map(|state| state.records.len())
        .unwrap_or(0)
}

// ── Config ────────────────────────────────────────────────────────────────────

pub(crate) struct S6Config {
    pub(crate) seed_events: u32,
    pub(crate) churn_cycles: usize,
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

// ── Churn-window driver ─────────────────────────────────────────────────────────
//
// The identical claim/release churn cycle for both phases. Each cycle claims the
// profile, awaits the resulting emit, releases it, then awaits the next emit —
// exercising the refs.profile projection on and off. No polling (D8):
// `configure_and_await_frame` blocks on the FrameProbe until the actor fires the
// update callback.

fn drive_churn_cycles(
    app: *mut NmpApp,
    churn_pubkey: &std::ffi::CStr,
    consumer_id: &std::ffi::CStr,
    cycles: usize,
    probe: &nmp_testing::harness_probe::FrameProbe,
    mut frame_count: impl FnMut() -> usize,
) {
    for _ in 0..cycles {
        // namespace=0 (Profile), shape=0 (ProfileRef), liveness=0 (CacheOk).
        nmp_app_resolve_ref(app, 0, churn_pubkey.as_ptr(), consumer_id.as_ptr(), 0, 0);
        configure_and_await_frame(app, probe, 250, &mut frame_count);

        nmp_app_release_ref(app, 0, churn_pubkey.as_ptr(), consumer_id.as_ptr());
        configure_and_await_frame(app, probe, 250, &mut frame_count);
    }
}

/// Snapshot the process-global churn counters, run the window, return the deltas
/// `(window_serialized, window_changed)` accumulated over the cycles.
fn run_churn_window(
    app: *mut NmpApp,
    churn_pubkey: &std::ffi::CStr,
    consumer_id: &std::ffi::CStr,
    cycles: usize,
    probe: &nmp_testing::harness_probe::FrameProbe,
    frame_count: impl FnMut() -> usize,
) -> (u64, u64) {
    let mut base_s = 0u64;
    let mut base_c = 0u64;
    nmp_app_read_projection_churn_stats(&mut base_s, &mut base_c);

    drive_churn_cycles(app, churn_pubkey, consumer_id, cycles, probe, frame_count);

    let mut end_s = 0u64;
    let mut end_c = 0u64;
    nmp_app_read_projection_churn_stats(&mut end_s, &mut end_c);
    (end_s.saturating_sub(base_s), end_c.saturating_sub(base_c))
}

// ── Frame-record percentile helpers ──────────────────────────────────────────

fn frame_bytes_percentiles(records: &[FrameRecord]) -> (u64, u64) {
    let mut sizes: Vec<u64> = records.iter().map(|r| r.frame_bytes as u64).collect();
    sizes.sort_unstable();
    (percentile_u64(&sizes, 50), percentile_u64(&sizes, 99))
}

/// p50 of the non-zero `serialize_us` samples (the first tick lags to 0).
fn serialize_us_p50(records: &[FrameRecord]) -> u64 {
    let mut sus: Vec<u64> = records
        .iter()
        .map(|r| r.serialize_us)
        .filter(|&v| v > 0)
        .collect();
    sus.sort_unstable();
    percentile_u64(&sus, 50)
}

fn phase_metrics(
    window_serialized: u64,
    window_changed: u64,
    records: &[FrameRecord],
) -> PhaseMetrics {
    let (p50_frame_bytes, p99_frame_bytes) = frame_bytes_percentiles(records);
    PhaseMetrics {
        window_serialized,
        window_changed,
        p50_frame_bytes,
        p99_frame_bytes,
        serialize_us_p50: serialize_us_p50(records),
        emit_count: records.len(),
    }
}

// ── Main scenario entry point ─────────────────────────────────────────────────

pub(crate) fn run(cfg: S6Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();

    let pubkeys = test_pubkeys(1);
    let churn_pubkey = &pubkeys[0];
    let consumer_id_a = std::ffi::CString::new("s6-churn-a").expect("valid CString");
    let consumer_id_b = std::ffi::CString::new("s6-churn-b").expect("valid CString");
    let base_ts: u64 = 1_700_000_000;

    // ── Phase A: baseline (incremental OFF) ──────────────────────────────────
    let (window_serialized_a, window_changed_a, records_a) = {
        let app_a: *mut NmpApp = nmp_app_new();
        let (signal_a, probe_a) = FrameProbe::new();
        let state_a = Box::new(Mutex::new(CallbackState {
            signal: signal_a,
            records: Vec::new(),
        }));
        let ctx_a = Box::into_raw(state_a) as *mut c_void;
        nmp_app_set_update_callback(app_a, ctx_a, Some(measure_cb));
        nmp_app_configure(app_a, 500, 12);
        inject_signed_events(app_a, base_ts, cfg.seed_events);
        let _ = configure_and_await_frame(app_a, &probe_a, cfg.settle_ms, || {
            callback_record_count(ctx_a)
        });

        let (ws, wc) = run_churn_window(app_a, churn_pubkey, &consumer_id_a, cfg.churn_cycles, &probe_a, || {
            callback_record_count(ctx_a)
        });

        nmp_app_set_update_callback(app_a, std::ptr::null_mut(), None);
        nmp_app_free(app_a);

        let boxed = unsafe { Box::from_raw(ctx_a as *mut Mutex<CallbackState>) };
        (ws, wc, boxed.into_inner().expect("lock").records)
    };

    // ── Phase B: incremental ON ───────────────────────────────────────────────
    let (window_serialized_b, window_changed_b, records_b, raw_frames_b) = {
        let app_b: *mut NmpApp = nmp_app_new();
        // ADR-0055 Rung 3 D3-2 — declare incremental-apply capability BEFORE
        // start. The kernel emits only Changed/Cleared rows from this point; the
        // first tick after declaration is a full baseline.
        let rc = nmp_app_declare_incremental_apply(app_b);
        assert_eq!(
            rc, 0,
            "nmp_app_declare_incremental_apply must return 0 (ok) before start; got rc={rc}"
        );

        let (signal_b, probe_b) = FrameProbe::new();
        let state_b = Box::new(Mutex::new(ByteCapture {
            signal: signal_b,
            records: Vec::new(),
            raw_frames: Vec::new(),
        }));
        let ctx_b = Box::into_raw(state_b) as *mut c_void;
        nmp_app_set_update_callback(app_b, ctx_b, Some(measure_cb_with_bytes));
        nmp_app_configure(app_b, 500, 12);
        inject_signed_events(app_b, base_ts, cfg.seed_events);
        let _ = configure_and_await_frame(app_b, &probe_b, cfg.settle_ms, || {
            byte_capture_record_count(ctx_b)
        });

        let (ws, wc) = run_churn_window(app_b, churn_pubkey, &consumer_id_b, cfg.churn_cycles, &probe_b, || {
            byte_capture_record_count(ctx_b)
        });

        nmp_app_set_update_callback(app_b, std::ptr::null_mut(), None);
        nmp_app_free(app_b);

        let boxed = unsafe { Box::from_raw(ctx_b as *mut Mutex<ByteCapture>) };
        let locked = boxed.into_inner().expect("lock");
        (ws, wc, locked.records, locked.raw_frames)
    };

    let wall_elapsed = wall_start.elapsed().as_secs_f64();

    // ── Byte-identity oracle ──────────────────────────────────────────────────
    // Replays Phase B's incremental frames through the ProjectionCache stand-in
    // and compares the reconstructed full set to Phase A's final full-frame set.
    let oracle = run_byte_identity_oracle(&raw_frames_b, &records_a);

    // ── Assemble metrics + apply the four capstone gates ──────────────────────
    let outcome = S6Outcome {
        seed_events: cfg.seed_events,
        churn_cycles: cfg.churn_cycles,
        phase_a: phase_metrics(window_serialized_a, window_changed_a, &records_a),
        phase_b: phase_metrics(window_serialized_b, window_changed_b, &records_b),
        oracle,
        wall_elapsed,
    };
    apply_gates(report, &outcome);
}
