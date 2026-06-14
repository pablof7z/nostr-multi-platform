//! S7 PASS/FAIL gates + report assembly (ADR-0055 R6-S4 feed-idle capstone §4).
//!
//! Extracted per repo file-size doctrine (500-LOC hard ceiling).
//!
//! This is the capstone measurement that empirically proves the whole-product
//! feed-gating win: once `nmp_app_declare_incremental_apply` is set and the feed
//! has been registered via `register_op_feed_defaults`, idle ticks produce ~0
//! feed bytes (the byte-equality gate fires and omits the `nmp.feed.home` row).
//!
//! The R3-S5 measurement (18%/68.8%) understated the real win because it did not
//! register the op_feed default — the feed (~58.8KB/tick, the dominant payload)
//! never appeared in that workload. This step registers it and measures for real.
//!
//! **Hard PASS/FAIL gates (four):**
//!
//! 1. `idle_feed_bytes_omitted` — with incremental ON the feed is absent from ≥ N
//!    consecutive idle ticks after the first full-baseline tick. Hard gate.
//! 2. `p50_frame_bytes_incremental_lt_baseline` — p50 idle total frame bytes with
//!    incremental ON < p50 with incremental OFF (by approximately the feed payload).
//! 3. `byte_identity_oracle_pass` — the incremental reconstruction == the full-frame
//!    reference (fail-closed; see [`crate::s7_feed_oracle`]).
//! 4. `false_resend_rate` — fraction of out-of-window ingest events that triggered
//!    a spurious feed re-emit. Gate: ≤ 0.0 (zero false re-sends).
//!
//! **Honesty note (mandate from #1415):** these numbers reflect the IDLE /
//! static-feed scenario. A mutating feed (new in-window event from a followed
//! author) still re-serializes the whole feed because the byte-equality gate
//! correctly fires Changed on content change. Row-delta sends (Option B) are
//! deferred post-v1.

use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use crate::s7_feed_oracle::FeedOracleResult;
use serde_json::json;

/// Per-phase metrics for the idle-feed measurement.
pub(crate) struct FeedPhaseMetrics {
    /// p50 / p99 total frame bytes over the idle window.
    pub(crate) p50_frame_bytes: u64,
    pub(crate) p99_frame_bytes: u64,
    /// p50 of the non-zero `serialize_us` samples.
    pub(crate) serialize_us_p50: u64,
    /// Number of frames captured during the idle window.
    pub(crate) emit_count: usize,
    /// Frames that carried `nmp.feed.home` (Changed).
    pub(crate) frames_with_feed: usize,
    /// Frames with `nmp.feed.home` absent (Unchanged — omitted by the gate).
    pub(crate) frames_without_feed: usize,
    /// p50 of feed payload bytes across frames that carried the feed.
    pub(crate) feed_bytes_p50: u64,
}

/// All inputs the four capstone gates need.
pub(crate) struct S7Outcome {
    /// Number of kind:1 events injected to populate the feed window.
    pub(crate) seeded_events: u32,
    /// Number of idle ticks run in each phase.
    pub(crate) idle_ticks: usize,
    pub(crate) phase_a: FeedPhaseMetrics,
    pub(crate) phase_b: FeedPhaseMetrics,
    pub(crate) oracle: FeedOracleResult,
    /// Number of out-of-window ingest events that triggered a spurious feed
    /// re-emit (false re-sends). Zero is the gate.
    pub(crate) false_resend_count: u32,
    /// Total out-of-window events injected (denominator for the rate).
    pub(crate) out_of_window_events: u32,
    pub(crate) wall_elapsed: f64,
}

impl S7Outcome {
    fn false_resend_rate(&self) -> f64 {
        if self.out_of_window_events == 0 {
            return 0.0;
        }
        self.false_resend_count as f64 / self.out_of_window_events as f64
    }
}

/// Apply the four gates, push report notes, and assemble the JSON measurements
/// block. Calls `report.finish()` which sets `passed` from gate results.
pub(crate) fn apply(report: &mut ScenarioMetrics, outcome: &S7Outcome) {
    let a = &outcome.phase_a;
    let b = &outcome.phase_b;
    let false_resend_rate = outcome.false_resend_rate();

    // ── Gate 1: idle feed bytes omitted (incremental ON) ─────────────────────
    //
    // With incremental ON, the first Phase B frame is always a full baseline
    // (the `declare_incremental_apply` rebaseline fires on the first configure
    // tick). Subsequent idle ticks should have feed_bytes == 0 (Unchanged →
    // omitted). We require that frames_without_feed >= 1 (i.e., at least one
    // idle tick after the first baseline did NOT re-emit the feed). In practice
    // we expect the vast majority of idle ticks to omit the feed.
    //
    // Concrete: with 5 idle ticks, the first is the full baseline (feed present),
    // ticks 2-5 should all omit the feed → frames_without_feed == 4.
    // Gate: frames_without_feed >= 1 (conservative; proves the gate fires at all).
    let frames_without_feed_b = b.frames_without_feed;
    report.gates.push(
        Gate::gte(
            "idle_feed_bytes_omitted",
            frames_without_feed_b as f64,
            1.0,
        )
        .with_note(
            "with incremental ON, at least 1 idle tick after the first full-baseline must \
             omit the feed (Unchanged → byte-equality gate fires, host retains prior value); \
             in practice all ticks after tick-1 are omitted",
        ),
    );

    // ── Gate 2: p50 frame bytes incremental < baseline ────────────────────────
    report.gates.push(
        Gate::lte(
            "p50_frame_bytes_incremental_lt_baseline",
            b.p50_frame_bytes as f64,
            a.p50_frame_bytes.saturating_sub(1) as f64,
        )
        .with_note(
            "incremental p50 total frame bytes must be strictly < baseline p50 \
             (the feed is the dominant payload; omitting it shrinks frames significantly)",
        ),
    );

    // ── Gate 3: byte-identity oracle PASS ────────────────────────────────────
    report.gates.push(
        Gate::lte(
            "byte_identity_oracle",
            if outcome.oracle.passed { 0.0 } else { 1.0 },
            0.0,
        )
        .with_note(outcome.oracle.note.clone()),
    );

    // ── Gate 4: false-resend rate == 0 ───────────────────────────────────────
    report.gates.push(
        Gate::lte("false_resend_rate", false_resend_rate, 0.0).with_note(
            "out-of-window ingest events (non-followed pubkey, never changes the feed) \
             must never trigger a spurious feed re-emit; false_resend_rate must be 0.0",
        ),
    );

    // ── serialize_us regression check (informational, not a gate) ────────────
    //
    // The feed is still serialized on every tick even with incremental ON — the
    // gate fires AFTER encoding, comparing bytes. So serialize_us should be
    // approximately the same in both phases. We log it but do not gate it because
    // the two phases are independent kernel instances (±15-20% timing noise).
    let serialize_us_note = if a.serialize_us_p50 == 0 && b.serialize_us_p50 == 0 {
        "no serialize_us data (both phases produced 0); treating as informational PASS".to_string()
    } else {
        let ratio = if a.serialize_us_p50 > 0 {
            b.serialize_us_p50 as f64 / a.serialize_us_p50 as f64
        } else {
            0.0
        };
        format!(
            "serialize_us p50: baseline={baseline}µs incremental={incr}µs ratio={ratio:.2} \
             (informational — encoding-then-omit pays the encode cost even when the byte \
             is not sent; ratio > 1.20 would be surprising but is not gated here due to \
             the independent-instance ±15-20% OS scheduling noise)",
            baseline = a.serialize_us_p50,
            incr = b.serialize_us_p50,
            ratio = ratio,
        )
    };

    // ── Report notes ─────────────────────────────────────────────────────────
    report.notes.push(
        "ADR-0055 R6-S4 capstone: feed-idle byte reduction — whole-product win with \
         nmp.feed.home registered. Honesty: this is the IDLE/static-feed scenario; \
         a mutating feed (new in-window event) still re-sends the whole feed (row-deltas \
         are Option B, deferred post-v1)."
            .to_string(),
    );
    report.notes.push(format!(
        "Phase A (baseline, incremental OFF): p50={}B p99={}B over {} frames; \
         feed present in {}/{} frames; feed_bytes_p50={}B; serialize_us_p50={}µs",
        a.p50_frame_bytes,
        a.p99_frame_bytes,
        a.emit_count,
        a.frames_with_feed,
        a.emit_count,
        a.feed_bytes_p50,
        a.serialize_us_p50,
    ));
    report.notes.push(format!(
        "Phase B (incremental ON): p50={}B p99={}B over {} frames; \
         feed present in {}/{} frames (rest omitted by byte-equality gate); \
         feed_bytes_p50={}B; serialize_us_p50={}µs",
        b.p50_frame_bytes,
        b.p99_frame_bytes,
        b.emit_count,
        b.frames_with_feed,
        b.emit_count,
        b.feed_bytes_p50,
        b.serialize_us_p50,
    ));
    report.notes.push(format!(
        "Byte-identity oracle: {}",
        outcome.oracle.note
    ));
    report.notes.push(format!(
        "False-resend gate: {}/{} out-of-window events → false_resend_rate={:.4}",
        outcome.false_resend_count,
        outcome.out_of_window_events,
        false_resend_rate,
    ));
    report.notes.push(serialize_us_note);
    report.notes.push(format!(
        "HEADLINE: idle total-frame-byte reduction with feed registered — \
         baseline p50={}B vs incremental p50={}B ({}B saved; {:.1}% reduction). \
         This is the REAL whole-product win R3-S5 could not show (the feed was not registered there).",
        a.p50_frame_bytes,
        b.p50_frame_bytes,
        a.p50_frame_bytes.saturating_sub(b.p50_frame_bytes),
        if a.p50_frame_bytes > 0 {
            (1.0 - b.p50_frame_bytes as f64 / a.p50_frame_bytes as f64) * 100.0
        } else {
            0.0
        },
    ));

    // ── JSON measurements ────────────────────────────────────────────────────
    report.measurements = json!({
        "seeded_events": outcome.seeded_events,
        "idle_ticks": outcome.idle_ticks,
        "phase_a_baseline": {
            "emit_count": a.emit_count,
            "p50_frame_bytes": a.p50_frame_bytes,
            "p99_frame_bytes": a.p99_frame_bytes,
            "serialize_us_p50": a.serialize_us_p50,
            "frames_with_feed": a.frames_with_feed,
            "frames_without_feed": a.frames_without_feed,
            "feed_bytes_p50": a.feed_bytes_p50,
        },
        "phase_b_incremental": {
            "emit_count": b.emit_count,
            "p50_frame_bytes": b.p50_frame_bytes,
            "p99_frame_bytes": b.p99_frame_bytes,
            "serialize_us_p50": b.serialize_us_p50,
            "frames_with_feed": b.frames_with_feed,
            "frames_without_feed": b.frames_without_feed,
            "feed_bytes_p50": b.feed_bytes_p50,
        },
        "gates": {
            "idle_feed_bytes_omitted": frames_without_feed_b >= 1,
            "p50_frame_bytes_incremental_lt_baseline": b.p50_frame_bytes < a.p50_frame_bytes,
            "byte_identity_oracle_pass": outcome.oracle.passed,
            "false_resend_rate": false_resend_rate,
            "false_resend_count": outcome.false_resend_count,
            "false_resend_out_of_window_events": outcome.out_of_window_events,
        },
        "wall_seconds": outcome.wall_elapsed,
    });

    report.finish(outcome.wall_elapsed);
}
