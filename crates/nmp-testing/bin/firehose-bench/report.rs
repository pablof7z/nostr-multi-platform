use serde::Serialize;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
pub(crate) struct FirehoseReport {
    pub(crate) tool: &'static str,
    pub(crate) status: &'static str,
    pub(crate) mode: &'static str,
    pub(crate) scale: &'static str,
    pub(crate) started_at_unix: u64,
    pub(crate) scenarios: Vec<ScenarioResult>,
    pub(crate) overall_passed: bool,
    pub(crate) limitations: Vec<String>,
    pub(crate) observations: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ScenarioResult {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) virtual_duration_seconds: u64,
    pub(crate) events_processed: u64,
    pub(crate) gates: Vec<GateResult>,
    pub(crate) metrics: ScenarioMetrics,
    pub(crate) passed: bool,
    pub(crate) observations: Vec<String>,
}

#[derive(Default, Serialize)]
pub(crate) struct ScenarioMetrics {
    pub(crate) first_item_ms: Option<f64>,
    pub(crate) filled_timeline_ms: Option<f64>,
    pub(crate) peak_memory_mb: Option<f64>,
    pub(crate) memory_drift_mb: Option<f64>,
    pub(crate) ingest_to_emit_p99_ms: Option<f64>,
    pub(crate) view_batch_hz: Option<f64>,
    pub(crate) max_deltas_per_view_sec: Option<f64>,
    pub(crate) dropped_events: Option<u64>,
    pub(crate) relay_connections: Option<u64>,
    pub(crate) open_close_dispatch_rate_per_sec: Option<f64>,
    pub(crate) mount_unmount_rate_per_sec: Option<f64>,
    pub(crate) projection_cache_hit_rate: Option<f64>,
    pub(crate) leaked_subscriptions: Option<u64>,
    pub(crate) detect_disconnect_p99_ms: Option<f64>,
    pub(crate) reconnect_p99_ms: Option<f64>,
    pub(crate) event_loss: Option<u64>,
    pub(crate) cross_account_bleed: Option<u64>,
    pub(crate) action_atomicity_failures: Option<u64>,
    pub(crate) per_account_memory_mb: Option<f64>,
    pub(crate) nip77_bytes_ratio: Option<f64>,
    pub(crate) req_only_bytes_mb: Option<f64>,
    pub(crate) nip77_bytes_mb: Option<f64>,
    pub(crate) decrypt_p99_ms: Option<f64>,
    pub(crate) nse_peak_memory_mb: Option<f64>,
    pub(crate) db_conflicts: Option<u64>,
    pub(crate) fd_growth: Option<i64>,
    pub(crate) panics: Option<u64>,
    pub(crate) trace_records: Option<u64>,
    pub(crate) synthetic_runtime_ms: Option<u128>,
}

#[derive(Serialize)]
pub(crate) struct GateResult {
    pub(crate) name: &'static str,
    pub(crate) measured: Option<f64>,
    pub(crate) budget: Option<f64>,
    pub(crate) passed: bool,
    pub(crate) note: Option<String>,
}

pub(crate) fn summarize_observations(report: &FirehoseReport) -> Vec<String> {
    if matches!(report.mode, "live") {
        return Vec::new();
    }
    vec![
        "Prototype replay gates pass, but they mainly validate the planned budgets and wrapper/cache lifecycle model, not real I/O behavior.".to_string(),
        "The highest-risk unmeasured surfaces are durable storage latency, real relay burstiness, UniFFI/platform-shadow memory, and actual NIP-77 support.".to_string(),
        "Next implementation step: replace synthetic storage/relay models with the real actor, relay adapter, storage backend, and generated wrapper shadow.".to_string(),
    ]
}

pub(crate) fn write_report(report: &FirehoseReport) -> io::Result<()> {
    let output_dir = PathBuf::from("docs/perf/firehose-bench");
    fs::create_dir_all(&output_dir)?;
    let stamp = report.started_at_unix.to_string();
    fs::write(
        output_dir.join(format!("{stamp}-{}.json", report.mode)),
        serde_json::to_string_pretty(report).expect("serializes report"),
    )?;
    fs::write(
        output_dir.join(format!("{stamp}-{}.md", report.mode)),
        markdown_report(report),
    )?;
    Ok(())
}

pub(crate) fn write_trace_manifest(report: &FirehoseReport) -> io::Result<()> {
    let output_dir = PathBuf::from("docs/perf/firehose-bench/traces");
    fs::create_dir_all(&output_dir)?;
    let trace = SyntheticTraceManifest {
        format: "synthetic-firehose-trace-manifest-v1",
        captured_at_unix: report.started_at_unix,
        scenario_count: report.scenarios.len(),
        total_records: report
            .scenarios
            .iter()
            .map(|scenario| scenario.events_processed)
            .sum(),
        note: "Prototype capture manifest; replace with frame-level relay capture once relay adapters exist.",
    };
    fs::write(
        output_dir.join(format!("{}-synthetic.json", report.started_at_unix)),
        serde_json::to_string_pretty(&trace).expect("serializes trace manifest"),
    )
}

#[derive(Serialize)]
pub(crate) struct SyntheticTraceManifest {
    pub(crate) format: &'static str,
    pub(crate) captured_at_unix: u64,
    pub(crate) scenario_count: usize,
    pub(crate) total_records: u64,
    pub(crate) note: &'static str,
}

pub(crate) fn markdown_report(report: &FirehoseReport) -> String {
    let mut out = String::new();
    out.push_str("# Firehose Bench Report\n\n");
    out.push_str(&format!("- Status: `{}`\n", report.status));
    out.push_str(&format!("- Mode: `{}`\n", report.mode));
    out.push_str(&format!("- Scale: `{}`\n", report.scale));
    out.push_str(&format!(
        "- Started at unix: `{}`\n",
        report.started_at_unix
    ));
    out.push_str(&format!(
        "- Overall passed: `{}`\n\n",
        report.overall_passed
    ));
    out.push_str("## Scenario Summary\n\n");
    out.push_str("| Scenario | Events | Duration | Passed | Key metrics |\n");
    out.push_str("|---|---:|---:|---|---|\n");
    for scenario in &report.scenarios {
        out.push_str(&format!(
            "| {} | {} | {}s | {} | {} |\n",
            scenario.name,
            scenario.events_processed,
            scenario.virtual_duration_seconds,
            scenario.passed,
            compact_metrics(&scenario.metrics)
        ));
    }
    out.push_str("\n## Limitations\n\n");
    for limitation in &report.limitations {
        out.push_str(&format!("- {limitation}\n"));
    }
    out.push_str("\n## Observations\n\n");
    for observation in &report.observations {
        out.push_str(&format!("- {observation}\n"));
    }
    out
}

pub(crate) fn compact_metrics(metrics: &ScenarioMetrics) -> String {
    let mut parts = Vec::new();
    if let Some(value) = metrics.first_item_ms {
        parts.push(format!("first_item={value:.2}ms"));
    }
    if let Some(value) = metrics.ingest_to_emit_p99_ms {
        parts.push(format!("ingest_p99={value:.2}ms"));
    }
    if let Some(value) = metrics.view_batch_hz {
        parts.push(format!("batch={value:.2}Hz"));
    }
    if let Some(value) = metrics.max_deltas_per_view_sec {
        parts.push(format!("deltas/view={value:.2}/s"));
    }
    if let Some(value) = metrics.memory_drift_mb {
        parts.push(format!("mem_drift={value:.2}MB"));
    }
    if let Some(value) = metrics.nip77_bytes_ratio {
        parts.push(format!("nip77_ratio={value:.4}"));
    }
    if let Some(value) = metrics.decrypt_p99_ms {
        parts.push(format!("decrypt_p99={value:.2}ms"));
    }
    if parts.is_empty() {
        "n/a".to_string()
    } else {
        parts.join(", ")
    }
}

pub(crate) fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
