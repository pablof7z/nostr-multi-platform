// ffi-transport-bench/bench_timing.rs
//
// Timing utilities: baselines, batch-timing pass, per-frame pass, and
// run_bucket_timing which orchestrates a single bucket measurement.
//
// METRIC LABELING: `time_batch` produces one sample per batch of K frames
// (mean of K deliveries).  Percentiles over these samples are BATCH-MEAN
// percentiles, NOT per-frame percentiles.  `time_per_frame` produces one
// sample per individual frame — true per-frame wall times.
// See the METRIC LABELING NOTE in report.rs for full discussion.

use super::config::{DELIVERIES_PER_BUCKET, PRNG_SEED, SMALL_BATCH_K, WARMUP_DELIVERIES};
use super::frames::make_frames;
use super::lanes::{uniffi_lane_deliver, UpdateFrameSink};
use super::report::BucketStats;
use nmp_native_runtime::UpdateListener;
use std::hint::black_box;
use std::time::Instant;

// ── Percentile computation ────────────────────────────────────────────────────

pub fn percentile(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((pct / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ── Baseline measurement ──────────────────────────────────────────────────────
//
// Two variants:
//
//   measure_baseline        — batch-of-K baseline for the batch timing passes.
//                             Returns (batch_time / K) as ns/frame.
//
//   measure_baseline_single — single-frame Instant overhead for per-frame
//                             timing passes (one Instant pair per delivery).
//                             Returns mean of `iters` single-frame timer calls
//                             with no transport work.
//
// Both are subtracted from their respective per-frame measurements.

pub fn measure_baseline(frames: &[Vec<u8>], iters: usize) -> u64 {
    for i in 0..1000 {
        black_box(frames[i % frames.len()].as_slice());
    }
    let k = SMALL_BATCH_K;
    let mut total_ns = 0u64;
    let mut count = 0usize;
    while count < iters {
        let t0 = Instant::now();
        for i in 0..k {
            black_box(frames[(count + i) % frames.len()].as_slice());
        }
        total_ns += t0.elapsed().as_nanos() as u64;
        count += k;
    }
    total_ns / count as u64
}

/// Measure the per-Instant-pair overhead for single-frame timing.
/// Returns the mean cost of `Instant::now() + black_box(index) + elapsed()`.
pub fn measure_baseline_single(frames: &[Vec<u8>], iters: usize) -> u64 {
    for i in 0..200 {
        let t0 = Instant::now();
        black_box(frames[i % frames.len()].as_slice());
        black_box(t0.elapsed());
    }
    let mut total_ns = 0u64;
    for i in 0..iters {
        let t0 = Instant::now();
        black_box(frames[i % frames.len()].as_slice());
        total_ns += t0.elapsed().as_nanos() as u64;
    }
    total_ns / iters as u64
}

// ── Generic timing passes ─────────────────────────────────────────────────────

/// Batch-mean timing pass for an arbitrary delivery closure.
///
/// Each returned sample is the MEAN of `k` consecutive deliveries.
/// p50/p95/p99 over these samples are BATCH-MEAN percentiles.
fn time_batch<F: FnMut(&[u8])>(
    mut deliver: F,
    frames: &[Vec<u8>],
    baseline_ns: u64,
    k: usize,
) -> Vec<u64> {
    let total = frames.len();
    let mut results = Vec::with_capacity(total / k + 1);
    for chunk_start in (0..total).step_by(k) {
        let chunk_end = (chunk_start + k).min(total);
        let chunk = &frames[chunk_start..chunk_end];
        let t0 = Instant::now();
        for frame in chunk {
            deliver(frame.as_slice());
        }
        let elapsed = t0.elapsed().as_nanos() as u64;
        let per_frame = elapsed / chunk.len() as u64;
        results.push(per_frame.saturating_sub(baseline_ns));
    }
    results
}

/// Per-frame timing pass for an arbitrary delivery closure.
///
/// Each returned sample is an individual frame timing.  Used only for
/// MEDIUM and LARGE buckets where per-frame cost is above Instant resolution.
fn time_per_frame<F: FnMut(&[u8])>(
    mut deliver: F,
    frames: &[Vec<u8>],
    baseline_single_ns: u64,
) -> Vec<u64> {
    let mut results = Vec::with_capacity(frames.len());
    for frame in frames {
        let t0 = Instant::now();
        deliver(frame.as_slice());
        let elapsed = t0.elapsed().as_nanos() as u64;
        results.push(elapsed.saturating_sub(baseline_single_ns));
    }
    results
}

// ── Bucket timing orchestrator ────────────────────────────────────────────────

/// Run one bucket timing pass.
///
/// `per_frame_p99` controls whether true per-frame p99 is also computed:
///   false — SMALL bucket (sub-us, near timer resolution; batch-mean only)
///   true  — MEDIUM/LARGE (above timer resolution; also time each frame alone)
pub fn run_bucket_timing(
    bucket: &'static str,
    min: usize,
    max: usize,
    seed_offset: u64,
    per_frame_p99: bool,
    c_listener: &UpdateListener,
    uniffi_sink: &dyn UpdateFrameSink,
) -> (BucketStats, BucketStats, u64) {
    let warmup = WARMUP_DELIVERIES;
    let iters = DELIVERIES_PER_BUCKET;
    let total = warmup + iters;
    let k = SMALL_BATCH_K;

    // Build frames: same PRNG seed for both lanes.
    let frames = make_frames(PRNG_SEED ^ seed_offset, total, min, max);

    // Empty-harness baseline (timer + index, no transport).
    let baseline_ns = measure_baseline(&frames, 10_000);

    // Warmup — both lanes
    for frame in &frames[..warmup] {
        c_listener(frame.as_slice());
    }
    for frame in &frames[..warmup] {
        uniffi_lane_deliver(uniffi_sink, frame.as_slice());
    }

    let measure_frames = &frames[warmup..];

    // Batch-mean timing passes.
    let mut c_batch = time_batch(|f| c_listener(f), measure_frames, baseline_ns, k);
    c_batch.sort_unstable();
    let mut u_batch = time_batch(
        |f| uniffi_lane_deliver(uniffi_sink, f),
        measure_frames,
        baseline_ns,
        k,
    );
    u_batch.sort_unstable();

    // Per-frame timing (MEDIUM/LARGE only).
    let (c_p99_pf, u_p99_pf) = if per_frame_p99 {
        let base_single = measure_baseline_single(measure_frames, 5_000);
        let mut c_pf = time_per_frame(|f| c_listener(f), measure_frames, base_single);
        c_pf.sort_unstable();
        let mut u_pf =
            time_per_frame(|f| uniffi_lane_deliver(uniffi_sink, f), measure_frames, base_single);
        u_pf.sort_unstable();
        (Some(percentile(&c_pf, 99.0)), Some(percentile(&u_pf, 99.0)))
    } else {
        (None, None)
    };

    let mean_bytes = frames[warmup..].iter().map(|f| f.len()).sum::<usize>() / iters;

    let c_stats = BucketStats {
        bucket,
        frame_bytes_min: min,
        frame_bytes_max: max,
        deliveries: iters,
        timing_batch_size: k,
        p50_batch_mean_ns: percentile(&c_batch, 50.0),
        p95_batch_mean_ns: percentile(&c_batch, 95.0),
        p99_batch_mean_ns: percentile(&c_batch, 99.0),
        p99_per_frame_ns: c_p99_pf,
    };
    let u_stats = BucketStats {
        bucket,
        frame_bytes_min: min,
        frame_bytes_max: max,
        deliveries: iters,
        timing_batch_size: k,
        p50_batch_mean_ns: percentile(&u_batch, 50.0),
        p95_batch_mean_ns: percentile(&u_batch, 95.0),
        p99_batch_mean_ns: percentile(&u_batch, 99.0),
        p99_per_frame_ns: u_p99_pf,
    };
    (c_stats, u_stats, mean_bytes as u64)
}
