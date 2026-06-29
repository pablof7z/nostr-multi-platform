// ffi-transport-bench/report.rs
//
// JSON + Markdown report structures and writer.

use crate::config::{
    COLLAPSE_SMALL_ABS_NS, COLLAPSE_THRESHOLD_NS, KEEP_THRESHOLD_NS,
    JNI_MANAGED_SURCHARGE_FACTOR, WEIGHT_LARGE, WEIGHT_MEDIUM, WEIGHT_SMALL,
};
use serde::Serialize;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Pre-registered decision verdict.
#[derive(Debug, Clone, Serialize)]
pub enum Verdict {
    /// Surcharged weighted-p99 delta < 5% of 16.67 ms AND UniFFI SMALL p99 < 250 us.
    /// Safe to collapse to a single UniFFI surface.
    Collapse,
    /// Surcharged weighted-p99 delta in [5%, 15%): insufficient synthetic evidence —
    /// escalate to a real Swift/Kotlin on-device A/B.
    Escalate,
    /// Surcharged weighted-p99 delta >= 15%: keep the internal C byte lane.
    KeepInternal,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Collapse => "COLLAPSE",
            Verdict::Escalate => "ESCALATE",
            Verdict::KeepInternal => "KEEP-INTERNAL",
        }
    }
}

/// Per-bucket timing statistics (nanoseconds per frame).
#[derive(Debug, Clone, Serialize)]
pub struct BucketStats {
    pub bucket: &'static str,
    pub frame_bytes_min: usize,
    pub frame_bytes_max: usize,
    pub deliveries: usize,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
}

/// Per-lane allocation statistics.
#[derive(Debug, Clone, Serialize)]
pub struct AllocStats {
    pub lane: &'static str,
    pub allocs_per_frame: f64,
    pub alloc_bytes_per_frame: f64,
}

/// Top-level bench report.
#[derive(Debug, Serialize)]
pub struct TransportBenchReport {
    pub tool: &'static str,
    pub started_at_unix: u64,
    /// rustc/opt-level/lto/codegen-units build metadata.
    pub build_info: BuildInfo,
    /// Timing results per lane per bucket.
    pub c_lane: LaneReport,
    pub uniffi_lane: LaneReport,
    /// Weighted-p99 delta (UniFFI minus C), raw nanoseconds.
    pub weighted_p99_delta_raw_ns: i64,
    /// Synthetic foreign-copy component (lower bound; see SYNTHETIC note).
    pub synthetic_foreign_copy_ns: i64,
    /// Surcharged delta: raw Rust-side delta + synthetic_foreign_copy * JNI_SURCHARGE.
    pub weighted_p99_delta_surcharged_ns: i64,
    /// As a fraction of the 60fps frame budget.
    pub surcharged_delta_pct_of_frame_budget: f64,
    /// Pre-registered verdict.
    pub verdict: String,
    /// Governing threshold used by the verdict.
    pub governing_threshold_ns: u64,
    /// Allocation pass results (empty if timing-only pass).
    pub alloc_stats: Vec<AllocStats>,
    /// Explicit synthetic-components caveat — always present.
    pub synthetic_caveat: Vec<String>,
    /// Additional notes.
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LaneReport {
    pub lane: &'static str,
    pub buckets: Vec<BucketStats>,
    pub weighted_p99_ns: f64,
    pub baseline_ns: u64,
}

#[derive(Debug, Serialize)]
pub struct BuildInfo {
    pub rustc_version: String,
    pub opt_level: String,
    pub debug_assertions: bool,
    pub target: String,
}

impl BuildInfo {
    pub fn capture() -> Self {
        BuildInfo {
            rustc_version: rustc_version_string(),
            opt_level: std::env::var("OPT_LEVEL").unwrap_or_else(|_| "unknown".to_string()),
            debug_assertions: cfg!(debug_assertions),
            target: std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string()),
        }
    }
}

fn rustc_version_string() -> String {
    // Use the CARGO_PKG_RUST_VERSION env var if set, else "unknown".
    // (The bench is built with --release; we record this for reproducibility.)
    std::env::var("RUSTC_VERSION")
        .unwrap_or_else(|_| "unknown (set RUSTC_VERSION env var to record)".to_string())
}

// ── Verdict computation ───────────────────────────────────────────────────────

/// Compute the pre-registered verdict from the surcharged delta and the
/// absolute UniFFI SMALL p99.
pub fn compute_verdict(
    surcharged_delta_ns: i64,
    uniffi_small_p99_ns: u64,
) -> Verdict {
    let delta = surcharged_delta_ns.max(0) as u64;
    if delta < COLLAPSE_THRESHOLD_NS && uniffi_small_p99_ns < COLLAPSE_SMALL_ABS_NS {
        Verdict::Collapse
    } else if delta >= KEEP_THRESHOLD_NS {
        Verdict::KeepInternal
    } else {
        Verdict::Escalate
    }
}

/// Compute the surcharged weighted-p99 delta.
///
/// SYNTHETIC CAVEAT: `synthetic_foreign_copy_ns` is a lower-bound estimate of
/// the managed-runtime cost (Swift Data construction + ARC bookkeeping, or
/// Kotlin ByteArray + GC). We model it as a plain memcpy+free of the weighted
/// mean frame size, which is the floor of the real cost. It is multiplied by
/// `JNI_MANAGED_SURCHARGE_FACTOR` (pre-registered = 3) to produce a
/// conservative estimate. The JNI boundary surcharge (GetByteArrayElements,
/// local-ref table) is NOT executed — only the multiplier is applied.
pub fn compute_surcharged_delta(
    c_weighted_p99: f64,
    uniffi_weighted_p99: f64,
    synthetic_foreign_copy_ns: i64,
) -> (i64, i64) {
    let raw_delta = (uniffi_weighted_p99 - c_weighted_p99) as i64;
    let surcharged_delta =
        raw_delta + synthetic_foreign_copy_ns * JNI_MANAGED_SURCHARGE_FACTOR as i64;
    (raw_delta, surcharged_delta)
}

/// Compute weighted p99 from per-bucket p99 values.
pub fn weighted_p99(small_p99: u64, medium_p99: u64, large_p99: u64) -> f64 {
    WEIGHT_SMALL * small_p99 as f64
        + WEIGHT_MEDIUM * medium_p99 as f64
        + WEIGHT_LARGE * large_p99 as f64
}

// ── Synthetic foreign-copy estimate ──────────────────────────────────────────

/// Estimate the synthetic foreign-copy component in nanoseconds.
///
/// Models the managed-runtime copy from RustBuffer into Swift Data / Kotlin
/// ByteArray as a single plain memcpy of the weighted mean frame size.
/// This is the LOWER BOUND of the real cost (no ARC/GC overhead, no
/// JNI table entries).
///
/// On modern Apple M-series / ARM processors, memcpy throughput is
/// approximately 20–40 GB/s, giving ~25–50 ns/KB. We use a conservative
/// 40 ns/KB baseline to stay below actual cost.
pub fn estimate_synthetic_foreign_copy_ns(weighted_mean_bytes: f64) -> i64 {
    // 40 ns per 1 KB = 40_000 ns per MB.
    let ns_per_byte: f64 = 40.0 / 1024.0;
    (weighted_mean_bytes * ns_per_byte) as i64
}

// ── Report file writer ────────────────────────────────────────────────────────

pub fn write_report(report: &TransportBenchReport) -> io::Result<()> {
    let output_dir = PathBuf::from("docs/perf/ffi-transport-bench");
    fs::create_dir_all(&output_dir)?;
    let stamp = report.started_at_unix;
    let json_path = output_dir.join(format!("{stamp}.json"));
    let md_path = output_dir.join(format!("{stamp}.md"));
    fs::write(
        &json_path,
        serde_json::to_string_pretty(report).expect("serializes report"),
    )?;
    fs::write(&md_path, markdown_report(report))?;
    Ok(())
}

fn markdown_report(report: &TransportBenchReport) -> String {
    let mut out = String::new();
    out.push_str("# FFI Transport Bench Report\n\n");
    out.push_str(&format!("- Tool: `{}`\n", report.tool));
    out.push_str(&format!("- Started at unix: `{}`\n", report.started_at_unix));
    out.push_str(&format!(
        "- Build: opt-level={}, debug-assertions={}, target={}, rustc={}\n\n",
        report.build_info.opt_level,
        report.build_info.debug_assertions,
        report.build_info.target,
        report.build_info.rustc_version,
    ));

    out.push_str("## Timing Results\n\n");
    out.push_str("| Lane | Bucket | Bytes (min–max) | p50 (ns) | p95 (ns) | p99 (ns) |\n");
    out.push_str("|------|--------|-----------------|----------|----------|----------|\n");
    for bucket in &report.c_lane.buckets {
        out.push_str(&format!(
            "| C-lane | {} | {}–{} | {} | {} | {} |\n",
            bucket.bucket,
            bucket.frame_bytes_min,
            bucket.frame_bytes_max,
            bucket.p50_ns,
            bucket.p95_ns,
            bucket.p99_ns,
        ));
    }
    for bucket in &report.uniffi_lane.buckets {
        out.push_str(&format!(
            "| UniFFI | {} | {}–{} | {} | {} | {} |\n",
            bucket.bucket,
            bucket.frame_bytes_min,
            bucket.frame_bytes_max,
            bucket.p50_ns,
            bucket.p95_ns,
            bucket.p99_ns,
        ));
    }
    out.push('\n');

    out.push_str("## Delta Analysis\n\n");
    out.push_str(&format!(
        "- Weighted-p99 C-lane: {:.0} ns\n",
        report.c_lane.weighted_p99_ns
    ));
    out.push_str(&format!(
        "- Weighted-p99 UniFFI: {:.0} ns\n",
        report.uniffi_lane.weighted_p99_ns
    ));
    out.push_str(&format!(
        "- Weighted-p99 delta (raw, Rust-side only): {} ns\n",
        report.weighted_p99_delta_raw_ns
    ));
    out.push_str(&format!(
        "- Synthetic foreign-copy component (lower bound): {} ns\n",
        report.synthetic_foreign_copy_ns
    ));
    out.push_str(&format!(
        "- Surcharged delta (raw + foreign-copy × {}): {} ns\n",
        JNI_MANAGED_SURCHARGE_FACTOR,
        report.weighted_p99_delta_surcharged_ns
    ));
    out.push_str(&format!(
        "- Surcharged delta as % of 16.67 ms budget: {:.2}%\n\n",
        report.surcharged_delta_pct_of_frame_budget * 100.0
    ));

    out.push_str(&format!(
        "## Verdict: **{}**\n\n",
        report.verdict
    ));
    out.push_str(&format!(
        "Pre-registered thresholds: COLLAPSE < {} ns ({:.0}% of budget), KEEP >= {} ns ({:.0}% of budget).\n\n",
        COLLAPSE_THRESHOLD_NS,
        5.0,
        KEEP_THRESHOLD_NS,
        15.0,
    ));

    if !report.alloc_stats.is_empty() {
        out.push_str("## Allocation Pass\n\n");
        out.push_str("| Lane | Allocs/frame | Bytes/frame |\n");
        out.push_str("|------|-------------|-------------|\n");
        for a in &report.alloc_stats {
            out.push_str(&format!(
                "| {} | {:.2} | {:.0} |\n",
                a.lane, a.allocs_per_frame, a.alloc_bytes_per_frame
            ));
        }
        out.push('\n');
    }

    out.push_str("## Synthetic Components Caveat\n\n");
    for caveat in &report.synthetic_caveat {
        out.push_str(&format!("- {caveat}\n"));
    }
    out.push('\n');

    if !report.notes.is_empty() {
        out.push_str("## Notes\n\n");
        for note in &report.notes {
            out.push_str(&format!("- {note}\n"));
        }
    }

    out
}

pub fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
