//! S7 — Feed-idle capstone (ADR-0055 R6-S4).
//!
//! **Purpose:** empirical PASS/FAIL proof that registering an app-owned OP feed via
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
//!   The byte-identity oracle needs ALL raw frames from Phase B start (including
//!   the settle baseline) to reconstruct incremental state. Metric percentiles
//!   use only idle frames. Window boundaries are tracked as record-count
//!   snapshots (`record_count()` before a window, `records_window(start, end)`
//!   after) — this composes cleanly across the idle window AND the two
//!   sequential false-resend probes.
//!
//! **False-resend probes (two, sequential — review BLOCKER fix):**
//!
//!   1. **Out-of-window FOLLOWED (the real over-invalidation proof).** Inject ONE
//!      VIEWER_PUBKEY (followed) event with `created_at = base_ts - 1` — older than
//!      every seeded event, so it lands BELOW the visible 80-card window. It passes
//!      `follow_set.predicate()` and mutates the engine's internal card set, but
//!      `snapshot(default-80)` is byte-identical → the byte-equality gate MUST omit
//!      it. This is Gate 4. A broken gate would re-emit → fail. A stranger event
//!      could NOT exercise this (it never reaches the engine — see probe 2).
//!   2. **Stranger (secondary predicate sanity check).** Inject non-followed
//!      STRANGER_PUBKEY events; rejected by the predicate before reaching the
//!      engine → feed trivially byte-identical. Proves the predicate filters, NOT
//!      that the byte-equality gate suppresses. Informational only (not the gate).
//!
//! **Metric honesty:** IDLE/static-feed scenario only. A new IN-WINDOW event
//! (followed, newest) still re-sends the whole feed (the gate correctly fires
//! Changed). Row-deltas (Option B) are deferred post-v1.
//!
//! D0: uses `actor_sender()` + `IngestPreVerifiedEvents` (test-support path).
//! D8: no polling; settle() is event-driven via `configure_and_await_frame`;
//! idle ticks use explicit configure() + wall-clock sleeps (genuine idle-tick
//! cadence under test — doctrine-allow: D8).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::ffi::{
    nmp_app_configure, nmp_app_declare_incremental_apply, nmp_app_free, nmp_app_new,
    nmp_app_set_update_callback, NmpApp,
};
use nmp_core::{decode_snapshot_envelope, decode_snapshot_typed_projections};
use nmp_testing::harness_probe::{FrameProbe, ProbeSignal};

use crate::common::{configure_and_await_frame, percentile_u64};
use crate::report::ScenarioMetrics;
use crate::s7_feed_events::{
    inject_events_from, inject_followed_reply_to_unknown_root, inject_stranger_replies,
    VIEWER_PUBKEY,
};
use crate::s7_feed_gates::{apply as apply_gates, FeedPhaseMetrics, S7Outcome};
use crate::s7_feed_oracle::{run_feed_oracle, FeedFrameRecord};

// ── Constants ────────────────────────────────────────────────────────────────

const FEED_KEY: &str = "nmp.testing.feed_idle";
const FEED_EVENT_COUNT: u32 = 120;
const IDLE_TICKS: usize = 8;
const TICK_SETTLE_MS: u64 = 600;
/// Over-invalidation probe events (the real proof): 1 FOLLOWED reply to a root
/// the engine never holds — parked in pending_attributions, surfaces no card,
/// leaves total_blocks unchanged → snapshot byte-identical → must be omitted.
const OUT_OF_WINDOW_FOLLOWED_EVENTS: u32 = 1;
/// Stranger (non-followed) probe events for the secondary predicate sanity check.
const OUT_OF_WINDOW_EVENTS: u32 = 20;

// ── Capture state ─────────────────────────────────────────────────────────────

/// All frames captured in a single phase run.
///
/// Window boundaries are tracked as record-count snapshots: the scenario reads
/// `record_count()` before a window, runs the window, and slices
/// `records_since(start)` afterward. This composes cleanly for the idle window
/// AND the two sequential false-resend probes (out-of-window-followed, stranger)
/// without bespoke per-window index fields.
struct CaptureState {
    /// Notifies the waiting [`FrameProbe`] on each captured frame.
    signal: ProbeSignal,
    /// ALL raw frames (for oracle replay of Phase B).
    oracle_raw_frames: Vec<Vec<u8>>,
    /// ALL decoded records.
    all_records: Vec<FeedFrameRecord>,
}

impl CaptureState {
    fn new(signal: ProbeSignal) -> Self {
        CaptureState {
            signal,
            oracle_raw_frames: Vec::new(),
            all_records: Vec::new(),
        }
    }

    /// Number of records captured so far (a window-start snapshot).
    fn record_count(&self) -> usize {
        self.all_records.len()
    }

    /// Records captured since the `start` snapshot.
    fn records_since(&self, start: usize) -> &[FeedFrameRecord] {
        &self.all_records[start.min(self.all_records.len())..]
    }

    /// Records in the half-open window `[start, end)`.
    fn records_window(&self, start: usize, end: usize) -> &[FeedFrameRecord] {
        let start = start.min(self.all_records.len());
        let end = end.min(self.all_records.len()).max(start);
        &self.all_records[start..end]
    }

    /// Count of feed-present frames captured since `start` (a false-resend tally).
    fn feed_resends_since(&self, start: usize) -> u32 {
        self.records_since(start)
            .iter()
            .filter(|r| r.feed_present)
            .count() as u32
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
        state.signal.notify();
    }
}

/// Read the current record count from a `*mut Mutex<CaptureState>` ctx pointer.
/// Used by the scenario to snapshot window boundaries.
///
/// SAFETY: `ctx` must be a live `Box::into_raw`-ed `Mutex<CaptureState>` pointer
/// that has not yet been reclaimed.
fn ctx_record_count(ctx: *mut std::ffi::c_void) -> usize {
    let ptr = ctx as *mut Mutex<CaptureState>;
    unsafe { (*ptr).lock() }
        .map(|s| s.record_count())
        .unwrap_or(0)
}

/// Count feed-present frames captured since the `start` record-count snapshot.
///
/// SAFETY: as [`ctx_record_count`].
fn ctx_record_count_resends(ctx: *mut std::ffi::c_void, start: usize) -> u32 {
    let ptr = ctx as *mut Mutex<CaptureState>;
    unsafe { (*ptr).lock() }
        .map(|s| s.feed_resends_since(start))
        .unwrap_or(0)
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

/// Event-driven settle: blocks until a frame arrives (seed events are ingested
/// and the first snapshot is emitted) or the 2500 ms deadline passes.
fn settle(app: *mut NmpApp, probe: &FrameProbe, ctx: *mut std::ffi::c_void) {
    configure_and_await_frame(app, probe, 2_500, || ctx_record_count(ctx));
}

fn run_idle_ticks(app: *mut NmpApp) {
    for _ in 0..IDLE_TICKS {
        nmp_app_configure(app, 500, 12);
        std::thread::sleep(Duration::from_millis(TICK_SETTLE_MS)); // doctrine-allow: D8 — idle-tick cadence under test
    }
}

// ── Config ────────────────────────────────────────────────────────────────────

pub(crate) struct S7Config {
    // `allow(dead_code)`: zero-sized reserved slot for future per-scenario
    // configuration; follows the Rust reserved-field pattern (underscore prefix).
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
        let _feed = nmp_native_runtime::register_op_feed_defaults(
            unsafe { &*app },
            VIEWER_PUBKEY.to_string(),
            vec![1],
            nmp_feed::ProjectionKey(FEED_KEY.to_string()),
        );

        let (signal_a, probe_a) = FrameProbe::new();
        let state = Box::new(Mutex::new(CaptureState::new(signal_a)));
        let ctx = Box::into_raw(state) as *mut std::ffi::c_void;
        nmp_app_set_update_callback(app, ctx, Some(capture_cb));

        // Seed events with id namespace [0, FEED_EVENT_COUNT) and timestamps
        // [base_ts, base_ts + FEED_EVENT_COUNT). The top-80 visible window is the
        // newest 80 (timestamps base_ts+40 .. base_ts+119).
        inject_events_from(app, VIEWER_PUBKEY, base_ts, 0, FEED_EVENT_COUNT);
        settle(app, &probe_a, ctx);

        // Idle window starts after settle.
        let idle_start = ctx_record_count(ctx);
        run_idle_ticks(app);
        let idle_end = ctx_record_count(ctx);

        nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
        nmp_app_free(app);

        let s = unsafe { Box::from_raw(ctx as *mut Mutex<CaptureState>) }
            .into_inner()
            .expect("lock");

        let idle: Vec<FeedFrameRecord> = clone_records(s.records_window(idle_start, idle_end));
        let all: Vec<FeedFrameRecord> = clone_records(&s.all_records);
        (idle, all)
    };

    // ── Phase B: incremental ON ───────────────────────────────────────────────
    let (idle_records_b, oracle_raw_frames_b, out_of_window_resends, stranger_resends) = {
        let app: *mut NmpApp = nmp_app_new();

        // Declare BEFORE the first configure tick (settle tick = first baseline).
        let rc = nmp_app_declare_incremental_apply(app);
        assert_eq!(
            rc, 0,
            "nmp_app_declare_incremental_apply must return 0 (ok); got {rc}"
        );

        let slot = unsafe { &*app }.active_account_handle();
        *slot.lock().expect("active-account slot") = Some(VIEWER_PUBKEY.to_string());

        let _feed = nmp_native_runtime::register_op_feed_defaults(
            unsafe { &*app },
            VIEWER_PUBKEY.to_string(),
            vec![1],
            nmp_feed::ProjectionKey(FEED_KEY.to_string()),
        );

        let (signal_b, probe_b) = FrameProbe::new();
        let state = Box::new(Mutex::new(CaptureState::new(signal_b)));
        let ctx = Box::into_raw(state) as *mut std::ffi::c_void;
        nmp_app_set_update_callback(app, ctx, Some(capture_cb));

        inject_events_from(app, VIEWER_PUBKEY, base_ts, 0, FEED_EVENT_COUNT);

        // Settle: oracle_raw_frames captures the first full-frame baseline.
        settle(app, &probe_b, ctx);

        // Idle window.
        let idle_start = ctx_record_count(ctx);
        run_idle_ticks(app);
        let idle_end = ctx_record_count(ctx);

        // ── Probe 1 (BLOCKER FIX): FOLLOWED reply to an unknown root ──────────
        //
        // A VIEWER_PUBKEY (followed) reply to a root the engine never holds. It
        // passes follow_set.predicate(), reaches the engine (Inserted → observer
        // fires), and MUTATES internal state (pending_attributions grows) — but
        // surfaces no card and does not touch the roots map / total_blocks, so the
        // serialized snapshot is byte-identical → the byte-equality gate MUST omit
        // it. This is the genuine "engine touched, output unchanged" proof. (An
        // out-of-window NEW root would NOT work — it bumps total_blocks/has_more →
        // snapshot legitimately changes → correct to re-emit; verified +160 B.)
        // id namespace 10_000 to avoid any seed collision.
        let oow_start = ctx_record_count(ctx);
        inject_followed_reply_to_unknown_root(app, base_ts - 1, 10_000);
        nmp_app_configure(app, 500, 12);
        std::thread::sleep(Duration::from_millis(TICK_SETTLE_MS)); // doctrine-allow: D8 — idle-tick cadence under test (false-resend probe)
        let oow_resends = ctx_record_count_resends(ctx, oow_start);

        // ── Probe 2 (secondary sanity): stranger REPLIES ─────────────────────
        //
        // Non-followed STRANGER_PUBKEY REPLIES (NOT roots — the OP-centric engine
        // surfaces all roots regardless of author, so stranger roots would
        // correctly change the feed). Reply-shaped events from a non-followed
        // author are dropped by the engine before any state change → feed
        // trivially byte-identical. Proves the predicate filters, NOT that the
        // byte-equality gate suppresses (would pass even with a broken gate).
        let stranger_start = ctx_record_count(ctx);
        inject_stranger_replies(app, base_ts + 200_000, 20_000, OUT_OF_WINDOW_EVENTS);
        nmp_app_configure(app, 500, 12);
        std::thread::sleep(Duration::from_millis(TICK_SETTLE_MS)); // doctrine-allow: D8 — idle-tick cadence under test (stranger probe)
        let stranger_resends = ctx_record_count_resends(ctx, stranger_start);

        nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
        nmp_app_free(app);

        let s = unsafe { Box::from_raw(ctx as *mut Mutex<CaptureState>) }
            .into_inner()
            .expect("lock");

        let idle: Vec<FeedFrameRecord> = clone_records(s.records_window(idle_start, idle_end));
        let oracle_frames: Vec<Vec<u8>> = s.oracle_raw_frames.clone();
        (idle, oracle_frames, oow_resends, stranger_resends)
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
        // Gate 4: the over-invalidation proof (1 followed out-of-window event).
        out_of_window_resend_count: out_of_window_resends,
        out_of_window_events: OUT_OF_WINDOW_FOLLOWED_EVENTS,
        // Secondary: stranger predicate sanity check.
        stranger_resend_count: stranger_resends,
        stranger_events: OUT_OF_WINDOW_EVENTS,
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
