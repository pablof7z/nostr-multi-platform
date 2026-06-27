//! S6 PASS/FAIL gates + report assembly (ADR-0055 Rung 3 capstone §9).
//!
//! Extracted from `s6_single_projection_churn.rs` per repo file-size doctrine
//! (500-LOC hard ceiling, split into cohesive submodules).
//!
//! Owns the four capstone gates, the human-readable report notes, and the JSON
//! `measurements` block. The measurement harness computes the raw numbers and
//! hands them here via [`PhaseMetrics`]; this module is the single place that
//! decides PASS/FAIL and the honesty framing (Tier-2 only; Tier-1 is a later
//! rung, codex Q4).

use crate::gate::Gate;
use crate::report::ScenarioMetrics;
use crate::s6_oracle::OracleResult;
use serde_json::json;

/// Per-phase churn-window measurements for one kernel instance.
pub(crate) struct PhaseMetrics {
    /// Tier-2 rows serialized over the churn window (post-omission for Phase B).
    pub(crate) window_serialized: u64,
    /// Tier-2 rows whose payload byte-hash changed over the window.
    pub(crate) window_changed: u64,
    /// p50 / p99 frame bytes over the window's emitted frames.
    pub(crate) p50_frame_bytes: u64,
    pub(crate) p99_frame_bytes: u64,
    /// p50 of the previous-tick `serialize_us` (encode time).
    pub(crate) serialize_us_p50: u64,
    /// Number of frames emitted during the window.
    pub(crate) emit_count: usize,
}

impl PhaseMetrics {
    /// Rows serialized that did not change (hash-based; informational only).
    fn wasted(&self) -> u64 {
        self.window_serialized.saturating_sub(self.window_changed)
    }

    /// Hash-based waste ratio (Rung-0 metric; informational, NOT a Rung-3 gate).
    fn waste_ratio(&self) -> f64 {
        if self.window_serialized > 0 {
            self.wasted() as f64 / self.window_serialized as f64
        } else {
            0.0
        }
    }
}

/// All inputs the capstone gates need: both phases + the oracle verdict.
pub(crate) struct S6Outcome {
    pub(crate) seed_events: u32,
    pub(crate) churn_cycles: usize,
    pub(crate) phase_a: PhaseMetrics,
    pub(crate) phase_b: PhaseMetrics,
    pub(crate) oracle: OracleResult,
    pub(crate) wall_elapsed: f64,
}

impl S6Outcome {
    /// Row suppression ratio — the CAPSTONE GATE metric for Rung 3.
    ///
    /// `1 - (serialized_b / serialized_a)`: the fraction of Tier-2 rows that
    /// full mode (Phase A) would have serialized but incremental mode (Phase B)
    /// suppressed via omit-Unchanged. This is the correct Rung-3 metric — unlike
    /// the hash-based `waste_ratio`, it directly measures fewer rows on the wire.
    ///
    /// The hash-based `waste_ratio` is NOT the Rung-3 gate: Phase B reads ~40% by
    /// it, but that residue is entirely the two Tier-1 (feed-class) keys
    /// `refs.event.envelopes` + `nip46_onboarding` — always-Changed by D3-7 (no
    /// manifest entry, never omitted), so they stay on the wire every tick and
    /// dominate the hash-waste. (Probe: `relay_diagnostics` is byte-identical only
    /// 1/103 ticks, so it is NOT the dominator.) Those Tier-1 keys are gated in a
    /// later rung; row suppression measures exactly the Tier-2 work this rung does.
    fn row_suppression_ratio(&self) -> f64 {
        let a = self.phase_a.window_serialized;
        if a > 0 {
            a.saturating_sub(self.phase_b.window_serialized) as f64 / a as f64
        } else {
            0.0
        }
    }

    /// serialize_us regression ceiling: baseline p50 × 1.20 (20% noise band).
    fn serialize_us_threshold(&self) -> u64 {
        if self.phase_a.serialize_us_p50 == 0 {
            0
        } else {
            (self.phase_a.serialize_us_p50 as f64 * 1.20).ceil() as u64
        }
    }
}

/// Build the four capstone gates, push report notes, and assemble the JSON
/// `measurements` block. Calls `report.finish()` (which sets `passed` from the
/// gate results).
pub(crate) fn apply(report: &mut ScenarioMetrics, outcome: &S6Outcome) {
    let a = &outcome.phase_a;
    let b = &outcome.phase_b;
    let row_suppression_ratio = outcome.row_suppression_ratio();
    let serialize_us_threshold = outcome.serialize_us_threshold();

    // ── Gate 1: Row suppression ratio ≥ 0.50 ────────────────────────────────
    // With incremental apply ON, at least half the Tier-2 rows that full mode
    // would have emitted are now suppressed (Unchanged = omitted). In the
    // single-projection churn workload (~3 of ~15 Tier-2 built-ins change per
    // cycle) we expect ~80% suppression; 0.50 is a conservative floor robust to
    // workload variation. Above 0.50 confirms the omit-Unchanged mechanism is live.
    report.gates.push(
        Gate::gte("row_suppression_ratio", row_suppression_ratio, 0.50).with_note(
            "Tier-2 single-projection churn: at least 50% of rows must be suppressed by \
             incremental-apply; ~80% expected. Tier-1 (feed) gating is a later rung.",
        ),
    );

    // ── Gate 2: incremental p50 frame bytes strictly < baseline p50 ──────────
    report.gates.push(
        Gate::lte(
            "p50_frame_bytes_incremental_vs_baseline",
            b.p50_frame_bytes as f64,
            a.p50_frame_bytes.saturating_sub(1) as f64,
        )
        .with_note("incremental p50 frame bytes must be strictly < baseline p50"),
    );

    // ── Gate 3: serialize_us p50 no regression (20% tolerance) ───────────────
    // Two independent OS-scheduled kernel instances carry inherent ±15–20%
    // timing noise; a strict equality gate would be flaky and meaningless. The
    // intent — "incremental apply adds no meaningful encode overhead" — is
    // checked against baseline p50 × 1.20. When both are 0 (no data), PASS.
    let gate_serialize_us = if a.serialize_us_p50 == 0 && b.serialize_us_p50 == 0 {
        Gate::lte("serialize_us_p50_no_regression", 0.0, 0.0)
            .with_note("no serialize_us data (all ticks produced 0); treating as PASS")
    } else {
        Gate::lte(
            "serialize_us_p50_no_regression",
            b.serialize_us_p50 as f64,
            serialize_us_threshold as f64,
        )
        .with_note(
            "incremental encode-time p50 must not exceed baseline p50 × 1.20 (20% tolerance \
             for CPU scheduling noise between independent kernel instances)",
        )
    };
    report.gates.push(gate_serialize_us);

    // ── Gate 4: byte-identity oracle PASS ────────────────────────────────────
    report.gates.push(
        Gate::lte(
            "byte_identity_oracle",
            if outcome.oracle.passed { 0.0 } else { 1.0 },
            0.0,
        )
        .with_note(outcome.oracle.note.clone()),
    );

    // ── Report notes ─────────────────────────────────────────────────────────
    report.notes.push(
        "ADR-0055 Rung 3 S5 capstone: Tier-2 single-projection churn waste → ~0; \
         Tier-1 (feed) gating is a later rung. Gate: row suppression ≥ 50%, \
         frame bytes strictly smaller, no encode-time regression, byte-identity oracle PASS."
            .to_string(),
    );
    report.notes.push(format!(
        "Phase A (baseline, incremental OFF): serialized={} changed={} wasted={} \
         waste_ratio={:.1}% (hash-based, informational)",
        a.window_serialized,
        a.window_changed,
        a.wasted(),
        a.waste_ratio() * 100.0,
    ));
    report.notes.push(format!(
        "Phase A frame bytes: p50={}B p99={}B over {} frames; serialize_us p50={}µs",
        a.p50_frame_bytes, a.p99_frame_bytes, a.emit_count, a.serialize_us_p50,
    ));
    report.notes.push(format!(
        "Phase B (incremental ON): serialized={} changed={} wasted={} \
         waste_ratio={:.1}% (hash-based, informational); \
         row_suppression_ratio={:.1}% (CAPSTONE GATE)",
        b.window_serialized,
        b.window_changed,
        b.wasted(),
        b.waste_ratio() * 100.0,
        row_suppression_ratio * 100.0,
    ));
    report.notes.push(format!(
        "Phase B frame bytes: p50={}B p99={}B over {} frames; serialize_us p50={}µs",
        b.p50_frame_bytes, b.p99_frame_bytes, b.emit_count, b.serialize_us_p50,
    ));
    report
        .notes
        .push(format!("Byte-identity oracle: {}", outcome.oracle.note));

    // ── JSON measurements ────────────────────────────────────────────────────
    report.measurements = json!({
        "seed_events": outcome.seed_events,
        "churn_cycles": outcome.churn_cycles,
        "phase_a_baseline": {
            "window_projections_serialized": a.window_serialized,
            "window_projections_changed": a.window_changed,
            "window_projections_wasted": a.wasted(),
            "waste_ratio_hash_based": a.waste_ratio(),
            "emit_count": a.emit_count,
            "p50_frame_bytes": a.p50_frame_bytes,
            "p99_frame_bytes": a.p99_frame_bytes,
            "serialize_us_p50": a.serialize_us_p50,
        },
        "phase_b_incremental": {
            "window_projections_serialized": b.window_serialized,
            "window_projections_changed": b.window_changed,
            "window_projections_wasted": b.wasted(),
            "waste_ratio_hash_based": b.waste_ratio(),
            "row_suppression_ratio": row_suppression_ratio,
            "emit_count": b.emit_count,
            "p50_frame_bytes": b.p50_frame_bytes,
            "p99_frame_bytes": b.p99_frame_bytes,
            "serialize_us_p50": b.serialize_us_p50,
        },
        "gates": {
            "row_suppression_ratio_gte_0.50": row_suppression_ratio >= 0.50,
            "p50_frame_bytes_incremental_lt_baseline": b.p50_frame_bytes < a.p50_frame_bytes,
            "serialize_us_p50_no_regression":
                b.serialize_us_p50 <= serialize_us_threshold
                    || (a.serialize_us_p50 == 0 && b.serialize_us_p50 == 0),
            "serialize_us_p50_threshold_20pct": serialize_us_threshold,
            "byte_identity_oracle_pass": outcome.oracle.passed,
        },
        "wall_seconds": outcome.wall_elapsed,
    });

    report.finish(outcome.wall_elapsed);
    // `finish()` calls `Gate::all_pass(&self.gates)` to set `passed`.
}
