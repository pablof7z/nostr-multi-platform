// ffi-transport-bench/config.rs
//
// Pre-registered decision rule and workload configuration.
//
// IMPORTANT: This file encodes the pre-registered decision rule BEFORE any
// measurement results are known. Do not modify the decision constants after
// running the bench; that would invalidate the pre-registration.
//
// Decision bands (pre-registered, see issue #2388):
//   Metric = weighted p99 of (UniFFI per-frame cost - C-lane per-frame cost),
//            with the synthetic foreign-copy component multiplied by
//            JNI_MANAGED_SURCHARGE_FACTOR before comparison.
//   Budget reference: 60fps render-frame = 16_666_667 ns.
//
//   NOTE: "weighted p99" is computed from per-bucket BATCH-MEAN p99 values
//   (percentiles of 1000-frame batch means).  See METRIC LABELING NOTE in
//   report.rs for implications.  The SMALL-bucket absolute gate also checks
//   the batch-mean p99 (labeled as COLLAPSE_SMALL_ABS_NS).
//
//   COLLAPSE  iff surcharged weighted-p99 delta < 5% of frame budget (< 833 us)
//             AND UniFFI SMALL batch-mean p99 < 250 us.
//   ESCALATE  iff surcharged weighted-p99 delta in [5%, 15%) of budget (833-2500 us).
//   KEEP      iff surcharged weighted-p99 delta >= 15% of budget (>= 2500 us).
//
//   No COLLAPSE verdict is permitted from synthetic evidence inside or above
//   the ESCALATE band.

// ── Frame budget ──────────────────────────────────────────────────────────────
/// One 60fps render frame in nanoseconds (the reference for all % bands).
pub const FRAME_BUDGET_NS: u64 = 16_666_667;

// ── Pre-registered decision thresholds ───────────────────────────────────────
/// 5% of frame budget — surcharged weighted-p99 delta below this → COLLAPSE.
pub const COLLAPSE_THRESHOLD_NS: u64 = (FRAME_BUDGET_NS as f64 * 0.05) as u64; // 833 333 ns

/// 15% of frame budget — surcharged weighted-p99 delta at or above this → KEEP.
pub const KEEP_THRESHOLD_NS: u64 = (FRAME_BUDGET_NS as f64 * 0.15) as u64; // 2 500 000 ns

/// Absolute UniFFI **batch-mean** p99 for the SMALL bucket must be below this
/// for COLLAPSE.  This checks `p99_batch_mean_ns` (percentile of per-1000-frame
/// means), not a true per-frame p99.  See the METRIC LABELING NOTE in report.rs.
pub const COLLAPSE_SMALL_ABS_NS: u64 = 250_000; // 250 us

/// Pre-registered conservative JNI/managed-runtime surcharge multiplier applied
/// to the synthetic foreign-copy component only (not to the Rust-side cost).
/// This models the JNI GetByteArrayElements / SetByteArrayRegion overhead and
/// Swift ARC bookkeeping that we do NOT execute in this in-process bench.
/// Setting it to 3 is the pre-registered lower bound from published JNI numbers.
pub const JNI_MANAGED_SURCHARGE_FACTOR: u64 = 3;

// ── Workload constants ────────────────────────────────────────────────────────
/// Number of deliveries per bucket per lane after warmup.
pub const DELIVERIES_PER_BUCKET: usize = 100_000;

/// Warmup deliveries discarded before measurement begins.
pub const WARMUP_DELIVERIES: usize = 10_000;

/// Batch size K for timing small-bucket calls (to stay above timer resolution).
/// p50/p95/p99 are derived by dividing batch time by K.
pub const SMALL_BATCH_K: usize = 1_000;

/// DEFAULT_EMIT_HZ from nmp-core (4 Hz steady cadence) — reproduced here as a
/// config constant so it can be referenced in the report without importing the
/// nmp-core crate at the bench level.
pub const STEADY_EMIT_HZ: u32 = 4;

/// Burst ceiling from app_lifecycle_ffi.rs clamp_emit_hz() — 12 Hz.
pub const BURST_EMIT_HZ: u32 = 12;

// ── Frame-size buckets (ADR-0070 omit-unchanged regime) ──────────────────────
/// SMALL bucket: 256 B – 2 KB (1–3 changed sidecar entries, incremental diff).
pub const SMALL_MIN_BYTES: usize = 256;
pub const SMALL_MAX_BYTES: usize = 2048;

/// MEDIUM bucket: 2 KB – 8 KB (batch/scroll diff).
pub const MEDIUM_MIN_BYTES: usize = 2048;
pub const MEDIUM_MAX_BYTES: usize = 8192;

/// LARGE bucket: 16 KB – 64 KB (full snapshot, account-switch / epoch bump,
/// visible-window = 80 items).
pub const LARGE_MIN_BYTES: usize = 16384;
pub const LARGE_MAX_BYTES: usize = 65536;

// ── Weighted mix ─────────────────────────────────────────────────────────────
/// Weight for SMALL bucket (80% of deliveries, dominant in omit-unchanged mode).
pub const WEIGHT_SMALL: f64 = 0.80;

/// Weight for MEDIUM bucket.
pub const WEIGHT_MEDIUM: f64 = 0.15;

/// Weight for LARGE bucket.
pub const WEIGHT_LARGE: f64 = 0.05;

// ── PRNG seed (shared across both lanes for byte-identical workload) ──────────
pub const PRNG_SEED: u64 = 0xdeadbeef_cafebabe;

// ── CLI ───────────────────────────────────────────────────────────────────────
pub struct Args {
    pub write_report: bool,
    pub fail_on_gate: bool,
    pub alloc_pass: bool,
}

impl Args {
    pub fn parse() -> Self {
        let mut write_report = true;
        let mut fail_on_gate = false;
        let mut alloc_pass = false;

        let mut iter = std::env::args().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--standard" => {}
                "--no-write-report" => write_report = false,
                "--fail-on-gate" => fail_on_gate = true,
                "--alloc-pass" => alloc_pass = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    eprintln!("unknown argument `{other}`");
                    print_help();
                    std::process::exit(64);
                }
            }
        }

        Self {
            write_report,
            fail_on_gate,
            alloc_pass,
        }
    }
}

pub fn print_help() {
    println!(
        "ffi-transport-bench [--standard] [--no-write-report] [--fail-on-gate] [--alloc-pass]"
    );
    println!();
    println!("Runs the UniFFI-vs-C-lane transport benchmark.");
    println!("Use --alloc-pass to run the allocation-counting pass (slower, no timing).");
}
