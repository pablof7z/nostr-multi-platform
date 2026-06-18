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
        let dir = PathBuf::from(format!("docs/perf/{}", self.run_id));
        fs::create_dir_all(&dir)?;
        let json = dir.join("sanity-report.json");
        fs::write(&json, serde_json::to_string_pretty(self).expect("serialize"))?;
        fs::write(dir.join("sanity-report.md"), self.markdown())?;
        Ok(json)
    }

    fn markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# New-Architecture Sanity Report\n\n");
        out.push_str(&format!("- tool: `{}`\n", self.tool));
        out.push_str(&format!("- mode: `{}`\n", self.mode));
        out.push_str(&format!("- run_id: `{}`\n", self.run_id));
        out.push_str(&format!("- relay: `{}`\n", self.relay));
        out.push_str(&format!("- started_at_unix: `{}`\n", self.started_at_unix));
        out.push_str(&format!("- overall_passed: `{}`\n\n", self.overall_passed));
        out.push_str("## Absolute gate results\n\n");
        out.push_str("| gate | phase | tool | hook | threshold | measured | verdict |\n");
        out.push_str("|---|---|---|---|---|---|---|\n");
        for r in &self.rows {
            out.push_str(&format!(
                "| {} | {} | `{}` | `{}` | {} | {} | **{}** |\n",
                r.gate,
                r.phase,
                r.tool,
                r.hook,
                r.threshold,
                r.measured.as_deref().unwrap_or("—"),
                r.verdict.as_str(),
            ));
        }
        if self.rows.iter().any(|r| r.note.is_some()) {
            out.push_str("\n### Notes\n\n");
            for r in self.rows.iter().filter(|r| r.note.is_some()) {
                out.push_str(&format!(
                    "- **{}** ({}): {}\n",
                    r.gate,
                    r.verdict.as_str(),
                    r.note.as_deref().unwrap_or(""),
                ));
            }
        }
        out.push_str("\n## Findings — hook gaps, stubs, blocked work\n\n");
        for f in &self.findings {
            out.push_str(&format!("- {f}\n"));
        }
        out
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
