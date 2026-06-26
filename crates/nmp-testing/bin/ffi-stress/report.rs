//! JSON metrics serializer and Markdown report writer.
//!
//! Output paths: `docs/perf/m10.5/<SCENARIO>/perf-report.{json,md}`
//!
//! Per `docs/retired/ffi-hardening-m10-5.md` R.2 and R.3. The on-disk schema uses
//! the unified [`nmp_testing::perf_report::PerfReport`] type, which is shared
//! across ffi-stress, firehose-bench, and sanity-gate.

use crate::gate::Gate;
use nmp_testing::perf_report::{self, GateVerdict, PerfGate, PerfReport, PerfScenario};
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Internal tracking accumulator for one scenario run.
///
/// Gathers [`Gate`]s and notes during the scenario, then converts to
/// [`PerfScenario`] for output via [`write_scenario_report`].
pub(crate) struct ScenarioMetrics {
    pub(crate) scenario: String,
    pub(crate) started_at_unix: u64,
    pub(crate) wall_seconds: f64,
    pub(crate) passed: bool,
    pub(crate) gates: Vec<Gate>,
    pub(crate) notes: Vec<String>,
    pub(crate) measurements: serde_json::Value,
}

impl ScenarioMetrics {
    pub(crate) fn new(scenario: impl Into<String>) -> Self {
        ScenarioMetrics {
            scenario: scenario.into(),
            started_at_unix: now_unix_seconds(),
            wall_seconds: 0.0,
            passed: false,
            gates: Vec::new(),
            notes: Vec::new(),
            measurements: serde_json::Value::Object(Default::default()),
        }
    }

    pub(crate) fn finish(&mut self, wall_seconds: f64) {
        self.wall_seconds = wall_seconds;
        self.passed = Gate::all_pass(&self.gates);
    }
}

/// Convert a ffi-stress [`Gate`] to the unified [`PerfGate`] output type.
fn gate_to_perf(g: &Gate) -> PerfGate {
    use crate::gate::GateOp;
    let threshold = match g.op {
        GateOp::Lte => format!("<= {}", g.threshold),
        GateOp::Gte => format!(">= {}", g.threshold),
        GateOp::Eq => format!("== {}", g.threshold),
    };
    let mut pg = PerfGate {
        name: g.name.clone(),
        threshold,
        measured: Some(format!("{:.4}", g.measured)),
        verdict: if g.passed { GateVerdict::Pass } else { GateVerdict::Fail },
        note: g.note.clone(),
    };
    if let Some(note) = &g.note {
        pg = pg.with_note(note.clone());
    }
    pg
}

/// Write unified `perf-report.json` and `perf-report.md` to
/// `docs/perf/m10.5/<scenario-prefix>/`.
///
/// The directory name is the scenario prefix (e.g. `S1`, `S2`) extracted from
/// the full scenario name like `S1-mount-unmount`. Per `ci.md` the bundle path
/// is `docs/perf/m10.5/S1/{perf-report.json,perf-report.md}`.
pub(crate) fn write_scenario_report(metrics: &ScenarioMetrics) -> io::Result<()> {
    let scenario_prefix = metrics
        .scenario
        .split('-')
        .next()
        .unwrap_or(&metrics.scenario)
        .to_uppercase();
    let dir = PathBuf::from(format!("docs/perf/m10.5/{scenario_prefix}"));

    let perf_gates: Vec<PerfGate> = metrics.gates.iter().map(gate_to_perf).collect();
    let scenario = PerfScenario::new(&metrics.scenario, metrics.wall_seconds, perf_gates)
        .with_notes(metrics.notes.clone())
        .with_measurements(metrics.measurements.clone());

    let run_id = format!("m10.5/{}", scenario_prefix.to_lowercase());
    let mut report = PerfReport::new("ffi-stress", run_id);
    report.push(scenario);

    perf_report::write(&report, &dir)
}

pub(crate) fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
