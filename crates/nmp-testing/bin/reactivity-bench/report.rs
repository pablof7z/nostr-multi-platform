use serde::Serialize;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
pub(crate) struct BenchReport {
    pub(crate) tool: &'static str,
    pub(crate) started_at_unix: u64,
    pub(crate) scale: &'static str,
    pub(crate) scenarios: Vec<ScenarioResult>,
    pub(crate) overall_passed: bool,
    pub(crate) caveats: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ScenarioResult {
    pub(crate) name: String,
    pub(crate) cached_events: usize,
    pub(crate) hot_events_resident: usize,
    pub(crate) replay_events: usize,
    pub(crate) event_rate_per_sec: u64,
    pub(crate) open_views: usize,
    pub(crate) interested_events: u64,
    pub(crate) candidate_view_hits: u64,
    pub(crate) false_wakeups: u64,
    pub(crate) candidates_per_delta: f64,
    pub(crate) false_wakeup_rate: f64,
    pub(crate) raw_deltas: u64,
    pub(crate) coalesced_deltas: u64,
    pub(crate) batches: u64,
    pub(crate) duration_ms: u128,
    pub(crate) lookup_p50_ns: u128,
    pub(crate) lookup_p99_ns: u128,
    pub(crate) recompute_p50_ns: u128,
    pub(crate) recompute_p99_ns: u128,
    pub(crate) max_batch_hz: f64,
    pub(crate) coalesced_deltas_per_sec: f64,
    pub(crate) max_deltas_per_view_per_sec: f64,
    pub(crate) estimated_working_set_memory_bytes: usize,
    pub(crate) steady_state_allocations_measured: bool,
    pub(crate) steady_state_events_measured: usize,
    pub(crate) steady_state_allocations: u64,
    pub(crate) steady_state_allocated_bytes: u64,
    pub(crate) steady_state_allocations_per_event: Option<f64>,
    pub(crate) steady_state_peak_heap_bytes: usize,
    pub(crate) gates: Vec<GateResult>,
    pub(crate) passed: bool,
    pub(crate) notes: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct GateResult {
    pub(crate) name: &'static str,
    pub(crate) measured: Option<f64>,
    pub(crate) budget: Option<f64>,
    pub(crate) passed: bool,
    pub(crate) note: Option<String>,
}

pub(crate) fn write_report(report: &BenchReport) -> io::Result<()> {
    let output_dir = PathBuf::from("docs/perf/reactivity-bench");
    fs::create_dir_all(&output_dir)?;
    let stamp = format!("{}-run-002", report.started_at_unix);
    let json_path = output_dir.join(format!("{stamp}.json"));
    let md_path = output_dir.join(format!("{stamp}.md"));
    fs::write(
        &json_path,
        serde_json::to_string_pretty(report).expect("serializes report"),
    )?;
    fs::write(&md_path, markdown_report(report))?;
    Ok(())
}

pub(crate) fn markdown_report(report: &BenchReport) -> String {
    let mut out = String::new();
    out.push_str("# Reactivity Bench Report\n\n");
    out.push_str(&format!("- Scale: `{}`\n", report.scale));
    out.push_str(&format!(
        "- Started at unix: `{}`\n",
        report.started_at_unix
    ));
    out.push_str(&format!(
        "- Overall passed: `{}`\n\n",
        report.overall_passed
    ));
    out.push_str("| Scenario | Lookup p99 | Recompute p99 | Batch Hz | Max delta/view/sec | Raw deltas | Coalesced deltas | Candidates/delta | False wake rate | Working-set memory | Allocs | Passed |\n");
    out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|\n");
    for scenario in &report.scenarios {
        let allocations = if scenario.steady_state_allocations_measured {
            scenario.steady_state_allocations.to_string()
        } else {
            "n/a".to_string()
        };
        out.push_str(&format!(
            "| {} | {} ns | {} ns | {:.2} | {:.2} | {} | {} | {:.2} | {:.2} | {} | {} | {} |\n",
            scenario.name,
            scenario.lookup_p99_ns,
            scenario.recompute_p99_ns,
            scenario.max_batch_hz,
            scenario.max_deltas_per_view_per_sec,
            scenario.raw_deltas,
            scenario.coalesced_deltas,
            scenario.candidates_per_delta,
            scenario.false_wakeup_rate,
            scenario.estimated_working_set_memory_bytes,
            allocations,
            scenario.passed
        ));
    }
    out.push_str("\n## Notes\n\n");
    for scenario in &report.scenarios {
        if scenario.notes.is_empty() {
            continue;
        }
        out.push_str(&format!("### {}\n\n", scenario.name));
        for note in &scenario.notes {
            out.push_str(&format!("- {note}\n"));
        }
        out.push('\n');
    }
    out.push_str("## Caveats\n\n");
    for caveat in &report.caveats {
        out.push_str(&format!("- {caveat}\n"));
    }
    out
}

pub(crate) fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
