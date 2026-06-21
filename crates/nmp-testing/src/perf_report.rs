//! Unified perf-output schema for all three harness tools: ffi-stress, firehose-bench,
//! and sanity-gate.
//!
//! Each tool builds a [`PerfReport`] (potentially with many [`PerfScenario`]s) and calls
//! [`write`] to emit `perf-report.{json,md}` to a caller-supplied directory. The on-disk
//! schema is tool-agnostic; the `tool` field distinguishes the source.
//!
//! # Design rationale
//!
//! All three harnesses previously had separate, structurally similar JSON/Markdown
//! emitters (`GateRow`/`Gate`/`GateResult`, `SanityReport`/`ScenarioMetrics`/`FirehoseReport`).
//! Consolidating them here:
//! - gives one authoritative JSON schema for the CI perf artefacts
//! - ensures the Markdown renderer is tested and maintained in one place
//! - removes the per-tool copy of `now_unix_seconds` / `iso_date`

use serde::Serialize;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single pass/fail gate row.
#[derive(Clone, Debug, Serialize)]
pub struct PerfGate {
    /// Short machine-readable gate name (e.g. `"idle-cpu"`, `"rev_monotonic"`).
    pub name: String,
    /// Human-readable threshold expression (e.g. `"<= 2.0 %"`, `"== 1"`).
    pub threshold: String,
    /// Measured value as a string, or `None` when the measurement could not be taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured: Option<String>,
    /// Whether the gate passed.
    pub passed: bool,
    /// Optional note: why SKIP/BLOCKED, justification, extra context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl PerfGate {
    /// Construct a numeric max-gate: PASS iff `measured <= threshold`.
    pub fn max(name: impl Into<String>, measured: f64, threshold: f64, unit: &str) -> Self {
        PerfGate {
            name: name.into(),
            threshold: format!("<= {threshold} {unit}"),
            measured: Some(format!("{measured:.4} {unit}")),
            passed: measured <= threshold,
            note: None,
        }
    }

    /// Construct a numeric min-gate: PASS iff `measured >= floor`.
    pub fn min(name: impl Into<String>, measured: f64, floor: f64, unit: &str) -> Self {
        PerfGate {
            name: name.into(),
            threshold: format!(">= {floor} {unit}"),
            measured: Some(format!("{measured:.4} {unit}")),
            passed: measured >= floor,
            note: None,
        }
    }

    /// Construct an equality gate: PASS iff `measured == expected`.
    pub fn eq(name: impl Into<String>, measured: f64, expected: f64) -> Self {
        PerfGate {
            name: name.into(),
            threshold: format!("== {expected}"),
            measured: Some(format!("{measured}")),
            passed: (measured - expected).abs() < f64::EPSILON,
            note: None,
        }
    }

    /// A gate that could not be measured (blocked / relay miss / hook gap).
    pub fn blocked(name: impl Into<String>, threshold: impl Into<String>, note: impl Into<String>) -> Self {
        PerfGate {
            name: name.into(),
            threshold: threshold.into(),
            measured: None,
            passed: false,
            note: Some(note.into()),
        }
    }

    /// Attach a note to this gate.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// One scenario's result within a [`PerfReport`].
#[derive(Clone, Debug, Serialize)]
pub struct PerfScenario {
    /// Scenario identifier (e.g. `"S1-mount-unmount"`, `"idle-soak"`, `"profile_thrashing"`).
    pub name: String,
    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wall time the scenario took.
    pub wall_seconds: f64,
    /// Whether all gates passed.
    pub passed: bool,
    /// Ordered gate results.
    pub gates: Vec<PerfGate>,
    /// Free-form notes (methodology remarks, caveats, measurement context).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Scenario-specific raw measurements, serialized as a JSON object.
    /// Use `serde_json::Value::Object(Default::default())` for none.
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub measurements: serde_json::Value,
}

impl PerfScenario {
    /// Construct a new scenario result.
    pub fn new(name: impl Into<String>, wall_seconds: f64, gates: Vec<PerfGate>) -> Self {
        let passed = gates.iter().all(|g| g.passed);
        PerfScenario {
            name: name.into(),
            description: None,
            wall_seconds,
            passed,
            gates,
            notes: Vec::new(),
            measurements: serde_json::Value::Null,
        }
    }

    /// Attach a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Attach notes.
    pub fn with_notes(mut self, notes: Vec<String>) -> Self {
        self.notes = notes;
        self
    }

    /// Attach raw measurements.
    pub fn with_measurements(mut self, v: serde_json::Value) -> Self {
        self.measurements = v;
        self
    }
}

/// Top-level perf report, emitted to disk by [`write`].
#[derive(Debug, Serialize)]
pub struct PerfReport {
    pub schema_version: u32,
    /// Which tool produced this report (`"ffi-stress"`, `"firehose-bench"`, `"sanity-gate"`).
    pub tool: String,
    /// Run identifier (e.g. `"m10.5"`, a timestamp, or a relay URL slug).
    pub run_id: String,
    pub started_at_unix: u64,
    /// All scenarios run in this invocation.
    pub scenarios: Vec<PerfScenario>,
    /// True iff every scenario passed.
    pub overall_passed: bool,
    /// Documented limitations, hook gaps, blocked rows.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
}

impl PerfReport {
    /// Create a new report for `tool` with `run_id`.
    pub fn new(tool: impl Into<String>, run_id: impl Into<String>) -> Self {
        PerfReport {
            schema_version: 1,
            tool: tool.into(),
            run_id: run_id.into(),
            started_at_unix: now_unix_seconds(),
            scenarios: Vec::new(),
            overall_passed: true,
            findings: Vec::new(),
        }
    }

    /// Push a scenario and update `overall_passed`.
    pub fn push(&mut self, scenario: PerfScenario) {
        if !scenario.passed {
            self.overall_passed = false;
        }
        self.scenarios.push(scenario);
    }

    /// Record a finding (limitation, hook gap, blocked measurement).
    pub fn finding(&mut self, msg: impl Into<String>) {
        self.findings.push(msg.into());
    }
}

/// Write `perf-report.json` and `perf-report.md` to `dir`.
///
/// Creates `dir` if it does not exist.
pub fn write(report: &PerfReport, dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(
        dir.join("perf-report.json"),
        serde_json::to_string_pretty(report).expect("serialize PerfReport"),
    )?;
    fs::write(dir.join("perf-report.md"), markdown(report))?;
    Ok(())
}

fn markdown(r: &PerfReport) -> String {
    let overall = if r.overall_passed { "PASS" } else { "FAIL" };
    let mut out = String::new();
    out.push_str(&format!(
        "# Perf Report — {} — {}\n\n",
        r.tool,
        iso_date(r.started_at_unix)
    ));
    out.push_str(&format!("- **run_id:** `{}`\n", r.run_id));
    out.push_str(&format!("- **started_at_unix:** `{}`\n", r.started_at_unix));
    out.push_str(&format!("- **overall:** **{overall}**\n\n"));

    for scenario in &r.scenarios {
        let s_result = if scenario.passed { "PASS" } else { "FAIL" };
        let pass_count = scenario.gates.iter().filter(|g| g.passed).count();
        let total = scenario.gates.len();
        out.push_str(&format!(
            "## {} — {} ({}/{} gates)\n\n",
            scenario.name, s_result, pass_count, total
        ));
        if let Some(desc) = &scenario.description {
            out.push_str(&format!("_{desc}_\n\n"));
        }
        out.push_str(&format!("- **wall_seconds:** {:.1}\n\n", scenario.wall_seconds));
        out.push_str("| Gate | Threshold | Measured | Result |\n");
        out.push_str("|---|---|---|---|\n");
        for gate in &scenario.gates {
            out.push_str(&format!(
                "| {} | {} | {} | **{}** |\n",
                gate.name,
                gate.threshold,
                gate.measured.as_deref().unwrap_or("—"),
                if gate.passed { "PASS" } else { "FAIL" },
            ));
        }
        out.push('\n');
        if !scenario.notes.is_empty() {
            out.push_str("### Notes\n\n");
            for note in &scenario.notes {
                out.push_str(&format!("- {note}\n"));
            }
            out.push('\n');
        }
        if !scenario.measurements.is_null() {
            out.push_str("### Raw measurements\n\n```json\n");
            out.push_str(&serde_json::to_string_pretty(&scenario.measurements).unwrap_or_default());
            out.push_str("\n```\n\n");
        }
    }

    if !r.findings.is_empty() {
        out.push_str("## Findings — limitations, gaps, blocked work\n\n");
        for f in &r.findings {
            out.push_str(&format!("- {f}\n"));
        }
        out.push('\n');
    }

    out
}

/// Current Unix timestamp in seconds.
pub fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn iso_date(unix: u64) -> String {
    let days = unix / 86400;
    let year = 1970 + days / 365;
    format!("{year}-xx-xx (unix {unix})")
}
