//! Sanity-report schema + writers.
//!
//! Emits `docs/perf/<run>/sanity-report.{json,md}` with the exact row schema:
//! `gate | phase | tool | hook | threshold | measured | verdict`.
//! Verdicts: PASS | FAIL | SKIP-relay-miss | BLOCKED.
//!
//! Honest-validation: a SKIP-relay-miss is a first-class outcome with a written
//! finding — never a faked green. BLOCKED marks a row that depends on unmerged
//! work (or a missing read hook) and could not be measured.

use serde::Serialize;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Verdict {
    #[serde(rename = "PASS")]
    Pass,
    #[serde(rename = "FAIL")]
    Fail,
    #[serde(rename = "SKIP-relay-miss")]
    SkipRelayMiss,
    #[serde(rename = "BLOCKED")]
    Blocked,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            Verdict::SkipRelayMiss => "SKIP-relay-miss",
            Verdict::Blocked => "BLOCKED",
        }
    }
}

/// One asserted gate row.
#[derive(Clone, Debug, Serialize)]
pub struct GateRow {
    pub gate: String,
    pub phase: String,
    /// The named capture tool (e.g. `ps -o rss`, `top -H`, `decode_snapshot_envelope`).
    pub tool: String,
    /// The in-process read hook or OS hook the measurement came from.
    pub hook: String,
    /// Human-readable threshold (e.g. `< 2.0 %`).
    pub threshold: String,
    /// Measured value, or `None` when not captured.
    pub measured: Option<String>,
    pub verdict: Verdict,
    /// Optional one-line note (why SKIP/BLOCKED, or extra context).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl GateRow {
    /// Construct a numeric max-gate row: PASS iff `measured <= threshold`.
    pub fn max(
        gate: &str,
        phase: &str,
        tool: &str,
        hook: &str,
        measured: f64,
        threshold: f64,
        unit: &str,
    ) -> Self {
        let verdict = if measured <= threshold {
            Verdict::Pass
        } else {
            Verdict::Fail
        };
        GateRow {
            gate: gate.to_string(),
            phase: phase.to_string(),
            tool: tool.to_string(),
            hook: hook.to_string(),
            threshold: format!("<= {threshold} {unit}"),
            measured: Some(format!("{measured:.2} {unit}")),
            verdict,
            note: None,
        }
    }

    /// Construct a numeric min-gate row: PASS iff `measured >= floor`.
    pub fn min(
        gate: &str,
        phase: &str,
        tool: &str,
        hook: &str,
        measured: f64,
        floor: f64,
        unit: &str,
    ) -> Self {
        let verdict = if measured >= floor {
            Verdict::Pass
        } else {
            Verdict::Fail
        };
        GateRow {
            gate: gate.to_string(),
            phase: phase.to_string(),
            tool: tool.to_string(),
            hook: hook.to_string(),
            threshold: format!(">= {floor} {unit}"),
            measured: Some(format!("{measured:.2} {unit}")),
            verdict,
            note: None,
        }
    }

    /// A row that could not be measured (relay miss / blocked / hook gap).
    pub fn unmeasured(
        gate: &str,
        phase: &str,
        tool: &str,
        hook: &str,
        threshold: &str,
        verdict: Verdict,
        note: &str,
    ) -> Self {
        GateRow {
            gate: gate.to_string(),
            phase: phase.to_string(),
            tool: tool.to_string(),
            hook: hook.to_string(),
            threshold: threshold.to_string(),
            measured: None,
            verdict,
            note: Some(note.to_string()),
        }
    }

    pub fn with_note(mut self, note: &str) -> Self {
        self.note = Some(note.to_string());
        self
    }
}

#[derive(Serialize)]
pub struct SanityReport {
    pub tool: &'static str,
    /// `local` (nak serve) or `live` (real public relays).
    pub mode: &'static str,
    pub run_id: String,
    pub relay: String,
    pub started_at_unix: u64,
    pub rows: Vec<GateRow>,
    /// Documented hook gaps + iOS/Android stubs + blocked-on-unmerged-work.
    pub findings: Vec<String>,
    pub overall_passed: bool,
}

impl SanityReport {
    pub fn new(mode: &'static str, run_id: String, relay: String) -> Self {
        SanityReport {
            tool: "sanity-gate",
            mode,
            run_id,
            relay,
            started_at_unix: now_unix(),
            rows: Vec::new(),
            findings: Vec::new(),
            overall_passed: true,
        }
    }

    pub fn push(&mut self, row: GateRow) {
        if matches!(row.verdict, Verdict::Fail) {
            self.overall_passed = false;
        }
        self.rows.push(row);
    }

    pub fn finding(&mut self, msg: impl Into<String>) {
        self.findings.push(msg.into());
    }

    pub fn write(&self) -> io::Result<PathBuf> {
        use nmp_testing::perf_report::{self, PerfGate, PerfReport, PerfScenario};

        let dir = PathBuf::from(format!("docs/perf/{}", self.run_id));

        // Build unified PerfReport from the sanity rows.
        let perf_gates: Vec<PerfGate> = self.rows.iter().map(|r| {
            let mut pg = if r.measured.is_some() {
                PerfGate {
                    name: format!("{} [{}]", r.gate, r.phase),
                    threshold: r.threshold.clone(),
                    measured: r.measured.clone(),
                    passed: matches!(r.verdict, Verdict::Pass),
                    note: r.note.clone(),
                }
            } else {
                PerfGate::blocked(
                    format!("{} [{}]", r.gate, r.phase),
                    r.threshold.clone(),
                    r.note.as_deref().unwrap_or("no measurement"),
                )
            };
            if let Some(note) = &r.note {
                pg = pg.with_note(note.clone());
            }
            pg
        }).collect();

        let scenario = PerfScenario::new(
            format!("{}-{}", self.mode, self.run_id),
            0.0,
            perf_gates,
        )
        .with_notes(self.findings.clone());

        let mut report = PerfReport::new(self.tool, self.run_id.clone());
        report.started_at_unix = self.started_at_unix;
        for f in &self.findings {
            report.finding(f.clone());
        }
        report.push(scenario);

        perf_report::write(&report, &dir)?;

        // Also write the legacy sanity-report.json for backward compat with
        // CI scripts that parse the old schema (removed once scripts migrate).
        let json = dir.join("sanity-report.json");
        fs::write(&json, serde_json::to_string_pretty(self).expect("serialize"))?;
        Ok(json)
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
