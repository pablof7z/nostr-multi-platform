//! S7 — Feed-idle capstone (ADR-0055 R6-S4).
//!
//! **Purpose:** empirical PASS/FAIL proof that registering `nmp.feed.home` via
//! `register_op_feed_defaults` + `nmp_app_declare_incremental_apply` reduces
//! idle-tick total frame bytes by ~58.8KB — the REAL whole-product win that
//! R3-S5 could not show because it did not register the op_feed default.
//!
//! **Design (§4 R6-S4 from #1415):**
//!
//! Phase A (baseline, incremental OFF):
//!   - Wire `register_op_feed_defaults` (viewer = VIEWER_PUBKEY).
//!   - Set active account slot to VIEWER_PUBKEY (self-inclusion).
//!   - Inject FEED_EVENT_COUNT kind:1 events from VIEWER_PUBKEY.
//!   - Settle until the feed stabilises, then run IDLE_TICKS configure ticks.
//!
//! Phase B (incremental ON):
//!   - `nmp_app_declare_incremental_apply` before the first configure tick.
//!   - Same seed + settle + idle sequence.
//!   - The settle tick is the first full-frame baseline; idle ticks omit the feed.
//!
//! **Capture structure:**
//!
//!   For Phase B, the byte-identity oracle needs ALL raw frames from phase start
//!   (including settle) to reconstruct incremental state. Metric percentiles use
//!   only idle frames. We track both windows with explicit index markers:
//!   `settle_start_idx` and `idle_start_idx` into a single `all_records` Vec.
//!
//! **False-resend probe:** After Phase B idle window, inject OUT_OF_WINDOW_EVENTS
//! from STRANGER_PUBKEY (not followed). Assert feed NOT re-emitted.
//!
//! **Metric honesty:** IDLE/static-feed scenario only. A new in-window event
//! still re-sends the whole feed. Row-deltas (Option B) are deferred post-v1.
//!
//! D0: uses `actor_sender()` + `IngestPreVerifiedEvents` (test-support path).
//! D8: no polling; idle ticks are explicit configure() calls + wall-clock sleeps.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use nmp_core::{decode_snapshot_envelope, decode_snapshot_typed_projections, ActorCommand};
use nmp_core::store::{RawEvent, VerifiedEvent};
use nmp_ffi::{
    nmp_app_configure, nmp_app_declare_incremental_apply, nmp_app_free, nmp_app_new,
    nmp_app_set_update_callback, NmpApp,
};

use crate::common::percentile_u64;
use crate::report::ScenarioMetrics;
use crate::s7_feed_gates::{apply as apply_gates, FeedPhaseMetrics, S7Outcome};
use crate::s7_feed_oracle::{run_feed_oracle, FeedFrameRecord};

// ── Constants ────────────────────────────────────────────────────────────────

const VIEWER_PUBKEY: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const STRANGER_PUBKEY: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const FEED_KEY: &str = "nmp.feed.home";
const FEED_EVENT_COUNT: u32 = 120;
const IDLE_TICKS: usize = 8;
const TICK_SETTLE_MS: u64 = 600;
const OUT_OF_WINDOW_EVENTS: u32 = 20;

// ── Event builders ────────────────────────────────────────────────────────────

fn make_event(pubkey: &str, created_at: u64, index: u64) -> VerifiedEvent {
    let id = format!("{:0>56x}{:0>8x}", 0u64, index as u32);
    let raw = RawEvent {
        id: id[..64].to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind: 1,
        tags: Vec::new(),
        content: format!("feed harness event {index} from {}", &pubkey[..8]),
        sig: "0".repeat(128),
    };
    VerifiedEvent::from_raw_unchecked(raw)
}

fn inject_events_from(app: *mut NmpApp, pubkey: &str, base_ts: u64, count: u32) {
    // SAFETY: app is a valid non-null pointer from nmp_app_new.
    let app_ref = unsafe { &*app };
    let events: Vec<VerifiedEvent> = (0..count as u64)
        .map(|i| make_event(pubkey, base_ts + i, i))
        .collect();
    app_ref
        .actor_sender()
        .send(ActorCommand::IngestPreVerifiedEvents(events))
        .ok();
}

// ── Capture state ─────────────────────────────────────────────────────────────

/// All frames captured in a single phase run.
/// `settle_start_idx`, `idle_start_idx`, and `probe_start_idx` are set by the
/// scenario code at the appropriate boundaries.
struct CaptureState {
    /// ALL raw frames (for oracle replay of Phase B).
    oracle_raw_frames: Vec<Vec<u8>>,
    /// ALL decoded records (indexed by the window markers).
    all_records: Vec<FeedFrameRecord>,
    /// Index of the first settle frame (0; set on construction).
    #[allow(dead_code)]
    settle_start_idx: usize,
    /// Index of the first idle frame (set after settle completes).
    idle_start_idx: usize,
    /// Index of the first probe frame (set after idle completes).
    probe_start_idx: usize,
}

impl CaptureState {
    fn new() -> Self {
        CaptureState {
            oracle_raw_frames: Vec::new(),
            all_records: Vec::new(),
            settle_start_idx: 0,
            idle_start_idx: 0,
            probe_start_idx: 0,
        }
    }

    fn mark_idle_start(&mut self) {
        self.idle_start_idx = self.all_records.len();
    }

    fn mark_probe_start(&mut self) {
        self.probe_start_idx = self.all_records.len();
    }

    fn idle_records(&self) -> &[FeedFrameRecord] {
        let start = self.idle_start_idx;
        // If probe_start was set AFTER idle records, bound to probe_start.
        let end = if self.probe_start_idx > start {
            self.probe_start_idx
        } else {
            self.all_records.len()
        };
        &self.all_records[start..end]
    }

    fn probe_records(&self) -> &[FeedFrameRecord] {
        &self.all_records[self.probe_start_idx..]
    }
}

extern "C" fn capture_cb(ctx: *mut std::ffi::c_void, payload: *const u8, payload_len: usize) {
    let ptr = ctx as *mut Mutex<CaptureState>;
    if let Ok(mut state) = unsafe { (*ptr).lock() } {
        if payload.is_null() || payload_len == 0 {
            return;
        }
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(payload, payload_len) };
        state.oracle_raw_frames.push(bytes.to_vec());
        state.all_records.push(decode_frame_record(bytes));
    }
}

fn decode_frame_record(bytes: &[u8]) -> FeedFrameRecord {
    let serialize_us = decode_snapshot_envelope(bytes)
        .map(|e| e.serialize_us)
        .unwrap_or(0);
    let projection_payloads = decode_snapshot_typed_projections(bytes)
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.key, p.payload))
        .collect();
    let feed_bytes = decode_snapshot_typed_projections(bytes)
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.key == FEED_KEY)
        .map(|p| p.payload.len())
        .unwrap_or(0);
    FeedFrameRecord {
        frame_bytes: bytes.len(),
        serialize_us,
        projection_payloads,
        feed_present: feed_bytes > 0,
        feed_bytes,
    }
}

// ── Metric helpers ────────────────────────────────────────────────────────────

fn phase_metrics(records: &[FeedFrameRecord]) -> FeedPhaseMetrics {
    let mut frame_sizes: Vec<u64> = records.iter().map(|r| r.frame_bytes as u64).collect();
    frame_sizes.sort_unstable();
    let p50 = percentile_u64(&frame_sizes, 50);
    let p99 = percentile_u64(&frame_sizes, 99);

    let mut sus: Vec<u64> = records
        .iter()
        .map(|r| r.serialize_us)
        .filter(|&v| v > 0)
        .collect();
    sus.sort_unstable();
    let sus_p50 = percentile_u64(&sus, 50);

    let mut feed_sizes: Vec<u64> = records
        .iter()
        .filter(|r| r.feed_present)
        .map(|r| r.feed_bytes as u64)
        .collect();
    feed_sizes.sort_unstable();
    let feed_p50 = percentile_u64(&feed_sizes, 50);

    FeedPhaseMetrics {
        p50_frame_bytes: p50,
        p99_frame_bytes: p99,
        serialize_us_p50: sus_p50,
        emit_count: records.len(),
        frames_with_feed: records.iter().filter(|r| r.feed_present).count(),
        frames_without_feed: records.iter().filter(|r| !r.feed_present).count(),
        feed_bytes_p50: feed_p50,
    }
}

fn settle(app: *mut NmpApp) {
    nmp_app_configure(app, 0, 500, 12);
    std::thread::sleep(Duration::from_millis(2_500));
}

fn run_idle_ticks(app: *mut NmpApp) {
    for _ in 0..IDLE_TICKS {
        nmp_app_configure(app, 0, 500, 12);
        std::thread::sleep(Duration::from_millis(TICK_SETTLE_MS));
    }
}

// ── Config ────────────────────────────────────────────────────────────────────

pub(crate) struct S7Config {
    #[allow(dead_code)]
    pub(crate) _reserved: (),
}

impl Default for S7Config {
    fn default() -> Self {
        S7Config { _reserved: () }
    }
}

// ── Main scenario ─────────────────────────────────────────────────────────────

pub(crate) fn run(_cfg: S7Config, report: &mut ScenarioMetrics) {
    let wall_start = Instant::now();
    let base_ts: u64 = 1_700_000_000;

    // ── Phase A: baseline (incremental OFF) ──────────────────────────────────
    let (idle_records_a, all_records_a) = {
        let app: *mut NmpApp = nmp_app_new();

        // Set active account: self-inclusion → viewer pubkey events qualify for feed.
        // SAFETY: valid non-null pointer from nmp_app_new.
        let slot = unsafe { &*app }.active_account_handle();
        *slot.lock().expect("active-account slot") = Some(VIEWER_PUBKEY.to_string());

        // Wire op_feed. SAFETY: valid pointer; called before start.
        let _feed = nmp_defaults::register_op_feed_defaults(
            unsafe { &*app },
            VIEWER_PUBKEY.to_string(),
        );

        let state = Box::new(Mutex::new(CaptureState::new()));
        let ctx = Box::into_raw(state) as *mut std::ffi::c_void;
        nmp_app_set_update_callback(app, ctx, Some(capture_cb));

        inject_events_from(app, VIEWER_PUBKEY, base_ts, FEED_EVENT_COUNT);
        settle(app);

        {
            let ptr = ctx as *mut Mutex<CaptureState>;
            unsafe { (*ptr).lock() }.expect("lock").mark_idle_start();
        }

        run_idle_ticks(app);

        nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
        nmp_app_free(app);

        let s = unsafe { Box::from_raw(ctx as *mut Mutex<CaptureState>) }
            .into_inner()
            .expect("lock");

        let idle: Vec<FeedFrameRecord> = clone_records(s.idle_records());
        let all: Vec<FeedFrameRecord> = clone_records(&s.all_records);
        (idle, all)
    };

    // ── Phase B: incremental ON ───────────────────────────────────────────────
    let (idle_records_b, oracle_raw_frames_b, false_resend_count) = {
        let app: *mut NmpApp = nmp_app_new();

        // Declare BEFORE the first configure tick (settle tick = first baseline).
        let rc = nmp_app_declare_incremental_apply(app);
        assert_eq!(
            rc, 0,
            "nmp_app_declare_incremental_apply must return 0 (ok); got {rc}"
        );

        let slot = unsafe { &*app }.active_account_handle();
        *slot.lock().expect("active-account slot") = Some(VIEWER_PUBKEY.to_string());

        let _feed = nmp_defaults::register_op_feed_defaults(
            unsafe { &*app },
            VIEWER_PUBKEY.to_string(),
        );

        let state = Box::new(Mutex::new(CaptureState::new()));
        let ctx = Box::into_raw(state) as *mut std::ffi::c_void;
        nmp_app_set_update_callback(app, ctx, Some(capture_cb));

        inject_events_from(app, VIEWER_PUBKEY, base_ts, FEED_EVENT_COUNT);

        // Settle: oracle_raw_frames captures the first full-frame baseline.
        settle(app);

        {
            let ptr = ctx as *mut Mutex<CaptureState>;
            unsafe { (*ptr).lock() }.expect("lock").mark_idle_start();
        }

        // Idle ticks: feed should be omitted (Unchanged → byte-equality gate).
        run_idle_ticks(app);

        // ── False-resend probe ────────────────────────────────────────────────
        {
            let ptr = ctx as *mut Mutex<CaptureState>;
            unsafe { (*ptr).lock() }.expect("lock").mark_probe_start();
        }
        inject_events_from(app, STRANGER_PUBKEY, base_ts + 200_000, OUT_OF_WINDOW_EVENTS);
        nmp_app_configure(app, 0, 500, 12);
        std::thread::sleep(Duration::from_millis(TICK_SETTLE_MS));

        nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
        nmp_app_free(app);

        let s = unsafe { Box::from_raw(ctx as *mut Mutex<CaptureState>) }
            .into_inner()
            .expect("lock");

        let false_resends = s
            .probe_records()
            .iter()
            .filter(|r| r.feed_present)
            .count() as u32;

        let idle: Vec<FeedFrameRecord> = clone_records(s.idle_records());
        let oracle_frames: Vec<Vec<u8>> = s.oracle_raw_frames.clone();
        (idle, oracle_frames, false_resends)
    };

    let wall_elapsed = wall_start.elapsed().as_secs_f64();

    // ── Byte-identity oracle ──────────────────────────────────────────────────
    //
    // Replays ALL Phase B raw frames (settle baseline + idle ticks) through the
    // ProjectionCache stand-in. The baseline tick seeds all keys; idle ticks add
    // nothing (Unchanged = omitted). The reconstructed end-state must match
    // Phase A's final full-frame record.
    let oracle = run_feed_oracle(&oracle_raw_frames_b, &all_records_a);

    // ── Assemble outcome + apply gates ────────────────────────────────────────
    let outcome = S7Outcome {
        seeded_events: FEED_EVENT_COUNT,
        idle_ticks: IDLE_TICKS,
        phase_a: phase_metrics(&idle_records_a),
        phase_b: phase_metrics(&idle_records_b),
        oracle,
        false_resend_count,
        out_of_window_events: OUT_OF_WINDOW_EVENTS,
        wall_elapsed,
    };
    apply_gates(report, &outcome);
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn clone_records(records: &[FeedFrameRecord]) -> Vec<FeedFrameRecord> {
    records
        .iter()
        .map(|r| FeedFrameRecord {
            frame_bytes: r.frame_bytes,
            serialize_us: r.serialize_us,
            projection_payloads: r.projection_payloads.clone(),
            feed_present: r.feed_present,
            feed_bytes: r.feed_bytes,
        })
        .collect()
}
