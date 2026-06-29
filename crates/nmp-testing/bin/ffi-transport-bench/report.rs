// ffi-transport-bench/report.rs
//
// JSON + Markdown report structures and writer.
//
// METRIC LABELING NOTE
// ====================
// The per-bucket timing statistics use a BATCH-MEAN approach for all three
// buckets: `timing_batch_size` frames are timed as a single batch, then the
// batch time is divided by the batch size to produce one per-frame mean
// sample.  The fields `p50_batch_mean_ns`, `p95_batch_mean_ns`, and
// `p99_batch_mean_ns` are percentiles over those batch-means, NOT over
// individual frame wall times.
//
// Averaging K frames per sample suppresses per-frame tail variation by
// roughly √K (standard error of the mean).  For K=1000 used in the SMALL
// bucket this is roughly 32×.  A "p99" of batch means therefore understates
// the actual per-frame tail fidelity by that factor.
//
// For SMALL bucket (sub-µs C-lane cost, near Instant timer resolution):
//   Batch-mean is the only reliable statistic; individual frame timing
//   would be dominated by Instant overhead (~50 ns per pair).
//   `p99_per_frame_ns` = None.
//
// For MEDIUM and LARGE buckets (hundreds of ns to µs — above timer resolution):
//   True per-frame p99 is ALSO computed by timing each frame individually
//   (100 k individual samples after warmup, separately baselined).
//   `p99_per_frame_ns` = Some(value).
//
// The verdict gate uses the SMALL-bucket batch-mean p99 (labeled explicitly
// as such in config.rs COLLAPSE_SMALL_ABS_NS and here in compute_verdict).

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
    /// Surcharged weighted-p99 delta < 5% of 16.67 ms AND UniFFI SMALL batch-mean p99 < 250 us.
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
///
/// IMPORTANT: `p50_batch_mean_ns`, `p95_batch_mean_ns`, `p99_batch_mean_ns`
/// are percentiles over PER-BATCH MEANS (batch size = `timing_batch_size`
/// frames per sample), NOT over individual frame wall times.  See the module
/// comment at the top of this file for the rationale and implications.
///
/// `p99_per_frame_ns` is set only for MEDIUM and LARGE buckets where
/// individual frame timing is above Instant resolution.
#[derive(Debug, Clone, Serialize)]
pub struct BucketStats {
    pub bucket: &'static str,
    pub frame_bytes_min: usize,
    pub frame_bytes_max: usize,
    pub deliveries: usize,
    /// Number of frames per timing sample.  All three batch-mean p-value fields
    /// are percentiles over (total_deliveries / timing_batch_size) samples.
    pub timing_batch_size: usize,
    /// p50 of per-batch-mean per-frame cost (ns).
    pub p50_batch_mean_ns: u64,
    /// p95 of per-batch-mean per-frame cost (ns).
    pub p95_batch_mean_ns: u64,
    /// p99 of per-batch-mean per-frame cost (ns).
    /// For SMALL bucket this is the only p99 available; tail fidelity is
    /// limited because 1000-frame averaging suppresses outliers by ~32x.
    pub p99_batch_mean_ns: u64,
    /// True per-frame p99: None for SMALL bucket (sub-us, below timer
    /// resolution); Some for MEDIUM/LARGE (above timer resolution).
    pub p99_per_frame_ns: Option<u64>,
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
    /// rustc/opt-level/target build metadata captured at compile time via build.rs.
    pub build_info: BuildInfo,
    /// Timing results per lane per bucket.
    pub c_lane: LaneReport,
    pub uniffi_lane: LaneReport,
    /// Weighted-p99 delta (UniFFI minus C), raw nanoseconds.
    /// NOTE: weighted_p99_ns in each LaneReport is computed from
    /// p99_batch_mean_ns values — see METRIC LABELING NOTE above.
    pub weighted_p99_delta_raw_ns: i64,
    /// Synthetic foreign-copy component (lower bound; see synthetic_caveat).
    pub synthetic_foreign_copy_ns: i64,
    /// Surcharged delta: raw Rust-side delta + synthetic_foreign_copy * JNI_SURCHARGE.
    pub weighted_p99_delta_surcharged_ns: i64,
    /// As a fraction of the 60fps frame budget.
    pub surcharged_delta_pct_of_frame_budget: f64,
    /// Pre-registered verdict.
    pub verdict: String,
    /// Explicit caveat for the COLLAPSE verdict — states the unverified assumption.
    pub verdict_caveat: String,
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
    /// Weighted p99 computed from per-bucket p99_batch_mean_ns values.
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
            // These are emitted by build.rs via `cargo:rustc-env=FFI_BENCH_*`.
            rustc_version: option_env!("FFI_BENCH_RUSTC_VERSION")
                .unwrap_or("unknown")
                .to_string(),
            opt_level: option_env!("FFI_BENCH_OPT_LEVEL")
                .unwrap_or("unknown")
                .to_string(),
            debug_assertions: cfg!(debug_assertions),
            target: option_env!("FFI_BENCH_TARGET")
                .unwrap_or("unknown")
                .to_string(),
        }
    }
}

// ── Verdict computation ───────────────────────────────────────────────────────

/// Compute the pre-registered verdict from the surcharged delta and the
/// absolute UniFFI SMALL **batch-mean** p99.
///
/// The SMALL-bucket gate uses `uniffi_small_batch_mean_p99_ns`, which is the
/// p99 of per-1000-frame means.  See the module comment for implications.
pub fn compute_verdict(
    surcharged_delta_ns: i64,
    uniffi_small_batch_mean_p99_ns: u64,
) -> Verdict {
    let delta = surcharged_delta_ns.max(0) as u64;
    if delta < COLLAPSE_THRESHOLD_NS && uniffi_small_batch_mean_p99_ns < COLLAPSE_SMALL_ABS_NS {
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

/// Compute weighted p99 from per-bucket **batch-mean** p99 values.
///
/// NOTE: these are batch-mean p99s, not per-frame p99s.  The weighted
/// combination inherits the same batch-mean semantics.
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
/// approximately 20-40 GB/s, giving ~25-50 ns/KB. We use a conservative
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
    out.push_str(
        "> **Metric note:** `p50/p95/p99 batch-mean (ns)` columns are percentiles over \
        per-1000-frame MEANS, not over individual frame wall times.  \
        Averaging 1000 frames suppresses tail variation by roughly 32x (CLT).  \
        `p99/frame` is a true per-frame p99 (100k individual samples); \
        reported for MEDIUM and LARGE only (SMALL is sub-us and below timer resolution).\n\n"
    );
    out.push_str("| Lane | Bucket | Bytes (min-max) | p50 batch-mean (ns) | p95 batch-mean (ns) | p99 batch-mean (ns) | p99/frame (ns) |\n");
    out.push_str("|------|--------|-----------------|---------------------|---------------------|---------------------|----------------|\n");
    for bucket in &report.c_lane.buckets {
        out.push_str(&format!(
            "| C-lane | {} | {}-{} | {} | {} | {} | {} |\n",
            bucket.bucket,
            bucket.frame_bytes_min,
            bucket.frame_bytes_max,
            bucket.p50_batch_mean_ns,
            bucket.p95_batch_mean_ns,
            bucket.p99_batch_mean_ns,
            bucket.p99_per_frame_ns
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a (sub-us)".to_string()),
        ));
    }
    for bucket in &report.uniffi_lane.buckets {
        out.push_str(&format!(
            "| UniFFI | {} | {}-{} | {} | {} | {} | {} |\n",
            bucket.bucket,
            bucket.frame_bytes_min,
            bucket.frame_bytes_max,
            bucket.p50_batch_mean_ns,
            bucket.p95_batch_mean_ns,
            bucket.p99_batch_mean_ns,
            bucket.p99_per_frame_ns
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a (sub-us)".to_string()),
        ));
    }
    out.push('\n');

    out.push_str("## Delta Analysis\n\n");
    out.push_str(
        "> Weighted-p99 values below are computed from per-bucket `p99_batch_mean_ns` \
        (see metric note above).\n\n"
    );
    out.push_str(&format!(
        "- Weighted-p99 C-lane (batch-mean): {:.0} ns\n",
        report.c_lane.weighted_p99_ns
    ));
    out.push_str(&format!(
        "- Weighted-p99 UniFFI (batch-mean): {:.0} ns\n",
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
        "- Surcharged delta (raw + foreign-copy x{}): {} ns\n",
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
    out.push_str(&format!(
        "> **Verdict caveat:** {}\n\n",
        report.verdict_caveat
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
