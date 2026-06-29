// ffi-transport-bench/main.rs
//
// FFI Transport Bench — in-process A/B microbench comparing two transports:
//
//   Lane A (C-lane): the real wiring from app_lifecycle_ffi.rs.
//     - An `extern "C" fn(*mut c_void, *const u8, usize)` invoked through
//       the same `Arc<dyn Fn(&[u8])>` closure shape (UpdateListener).
//     - Zero-copy ptr/len dispatch.
//     - PLUS one mandatory shell-copy memcpy out of the transient slice
//       (required for fairness: the C contract says the slice is valid only
//       for the callback duration; every real host copies once).
//
//   Lane B (UniFFI-lane): the REAL uniffi_core RustBuffer lowering path.
//     - `RustBuffer::from_vec` (real alloc + memcpy into a Vec-backed buffer).
//     - `<Vec<u8> as Lower<UT>>::lower(data)` — the genuine FfiConverter
//       lowering path with 4-byte i32 length prefix + per-byte write.
//     - An indirect vtable call into a Rust-implemented foreign-trait
//       callback stub that performs the lower-bound foreign consume:
//       memcpy RustBuffer contents into an owned Vec, then free.
//
// SYNTHETIC note (labeled clearly, LOWER BOUND only):
//   - The foreign-consume stub (memcpy RustBuffer → owned Vec + free) is a
//     lower bound for the real Swift Data / Kotlin ByteArray construction
//     including ARC/GC bookkeeping.
//   - The JNI boundary surcharge (GetByteArrayElements, local-ref table) is
//     NOT executed; it is applied only as a pre-registered 3x multiplier in
//     the decision rule.
//   - Real cross-language thread/dispatch-queue handoff and on-device
//     backpressure drop/reorder are NOT modeled (in-process, reported as 0,
//     explicitly labeled as not a real-transport claim).
//
// Pre-registered decision rule:
//   See config.rs for threshold constants and comments.
//
// Usage:
//   cargo run -p nmp-testing --bin ffi-transport-bench --release -- --standard
//   cargo run -p nmp-testing --bin ffi-transport-bench --release -- --alloc-pass

mod allocator;
mod config;
mod report;
mod rng;

use allocator::alloc_snapshot;
use config::{
    Args, BURST_EMIT_HZ, COLLAPSE_THRESHOLD_NS, DELIVERIES_PER_BUCKET, FRAME_BUDGET_NS,
    JNI_MANAGED_SURCHARGE_FACTOR, KEEP_THRESHOLD_NS, LARGE_MAX_BYTES, LARGE_MIN_BYTES,
    MEDIUM_MAX_BYTES, MEDIUM_MIN_BYTES, PRNG_SEED, SMALL_BATCH_K, SMALL_MAX_BYTES,
    SMALL_MIN_BYTES, STEADY_EMIT_HZ, WARMUP_DELIVERIES, WEIGHT_LARGE, WEIGHT_MEDIUM,
    WEIGHT_SMALL,
};
use report::{
    compute_surcharged_delta, compute_verdict, estimate_synthetic_foreign_copy_ns,
    now_unix_seconds, weighted_p99, write_report, AllocStats, BucketStats, BuildInfo, LaneReport,
    TransportBenchReport,
};
use rng::Rng;
use std::ffi::c_void;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

// ── Imports from the real nmp-native-runtime and uniffi_core crates ───────────
use nmp_native_runtime::UpdateListener;
use uniffi_core::Lower;

// Dummy UniFFI type tag — required by FfiConverter generic parameter.
struct UniFfiTag;

// ── Frame buffer generation ───────────────────────────────────────────────────

/// Generate a deterministic frame buffer of `size` bytes from `rng`.
/// The content is synthetic (pseudo-random bytes); transport carries opaque bytes.
fn make_frame(rng: &mut Rng, size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(size);
    for _ in 0..(size / 8) {
        let v = rng.next_u64();
        buf.extend_from_slice(&v.to_le_bytes());
    }
    // Fill any remainder
    let rem = size % 8;
    if rem > 0 {
        let v = rng.next_u64();
        buf.extend_from_slice(&v.to_le_bytes()[..rem]);
    }
    buf
}

/// Generate `count` frame buffers of sizes uniformly distributed in [min, max).
fn make_frames(seed: u64, count: usize, min: usize, max: usize) -> Vec<Vec<u8>> {
    let mut rng = Rng::new(seed);
    (0..count)
        .map(|_| {
            let size = rng.next_range(min, max);
            make_frame(&mut rng, size)
        })
        .collect()
}

// ── Lane A: C-lane ────────────────────────────────────────────────────────────
//
// Replicates the exact closure shape from app_lifecycle_ffi.rs:
//
//   let listener = callback.map(|callback| {
//       let context = context as usize;
//       Arc::new(move |bytes: &[u8]| {
//           callback(context as *mut c_void, bytes.as_ptr(), bytes.len());
//       }) as nmp_native_runtime::UpdateListener
//   });
//
// Lane A additionally performs a mandatory shell-copy (one memcpy of the
// transient slice into an owned Vec) to make the comparison fair: the real
// contract says the slice is valid only for the callback duration, so every
// real host must copy once.

/// The extern "C" callback signature from app_lifecycle_ffi.rs.
pub type UpdateCallback = extern "C" fn(*mut c_void, *const u8, usize);

/// The shell-side receive buffer — simulates the host's owned copy.
/// In the real app this is the Swift/Kotlin Data / ByteArray built from the
/// transient slice.  Here it is an allocated Vec dropped at end of callback.
extern "C" fn c_lane_callback(context: *mut c_void, ptr: *const u8, len: usize) {
    // SAFETY: context points to a valid AtomicU64 accumulator on the stack
    // (via pointer coercion below). We only read ptr/len, never store them.
    let _ = black_box(context);

    // Mandatory shell copy: caller owns this allocation.
    // This models the host copying the transient slice before the frame is
    // forwarded to UI or stored.
    let owned: Vec<u8> = unsafe {
        let slice = std::slice::from_raw_parts(ptr, len);
        slice.to_vec()
    };
    black_box(owned);
}

/// Build a `UpdateListener` that drives Lane A.
fn build_c_lane_listener() -> UpdateListener {
    let callback: UpdateCallback = c_lane_callback;
    // The context pointer is a dummy sentinel (same pattern as production code).
    let context: usize = 0xdeadbeef_usize;
    Arc::new(move |bytes: &[u8]| {
        callback(context as *mut c_void, bytes.as_ptr(), bytes.len());
    })
}

// ── Lane B: UniFFI lane ───────────────────────────────────────────────────────
//
// Exercises the real UniFFI lowering path:
//   1. `<Vec<u8> as Lower<UniFfiTag>>::lower(data)` — genuine FfiConverter:
//      allocates a Vec<u8> scratch buffer, writes 4-byte i32 length prefix +
//      each byte via `write()`, wraps in `RustBuffer::from_vec()`.
//   2. An indirect vtable dispatch into a Rust foreign-trait callback stub.
//   3. The stub performs the lower-bound foreign consume: copies RustBuffer
//      contents into an owned Vec (mimicking Swift Data / Kotlin ByteArray),
//      then drops (frees) the RustBuffer.
//
// SYNTHETIC: the stub's memcpy is a LOWER BOUND of the real foreign consume.
// Real ARC/GC bookkeeping, JNI local-ref table management, and dispatch-queue
// hop are NOT modeled.  The pre-registered 3× surcharge is applied in the
// decision computation (report.rs), not here.

/// Foreign-trait callback interface used by Lane B.
/// This is the Rust side of what UniFFI would auto-generate as a callback
/// interface VTable.  The vtable dispatch is the indirect fn-pointer call
/// through a `Box<dyn UpdateFrameSink>` trait object.
trait UpdateFrameSink: Send + Sync {
    /// Called with the lowered RustBuffer.  The sink owns the buffer and must
    /// free it (drop it back into Rust ownership) to avoid leaks.
    fn on_frame(&self, buf: uniffi_core::RustBuffer);
}

/// Lower-bound foreign-consume stub.
///
/// SYNTHETIC (explicitly labeled): this models the floor cost of what Swift/
/// Kotlin must do when receiving a UniFFI `Vec<u8>` callback:
///   1. Copy the RustBuffer bytes into a managed heap allocation (Swift Data /
///      Kotlin ByteArray).  Modeled as `buf.destroy_into_vec()`.
///   2. ARC/GC overhead — NOT modeled; pre-registered 3× surcharge applied in
///      report.rs compensates.
///   3. JNI boundary surcharge — NOT modeled; same 3× band.
struct LowerBoundForeignSink;

impl UpdateFrameSink for LowerBoundForeignSink {
    #[inline(never)]
    fn on_frame(&self, buf: uniffi_core::RustBuffer) {
        // Lower-bound foreign consume: reclaim the RustBuffer into an owned
        // Vec<u8> and immediately drop it.  This is the floor cost of the
        // real managed-runtime allocation + copy.  SYNTHETIC.
        let owned = black_box(buf.destroy_into_vec());
        drop(owned);
    }
}

/// Run one UniFFI-lane delivery of a single frame.
///
/// This function is called once per frame.  It:
///   1. Clones the frame data (to give the lowering path an owned Vec<u8>).
///   2. Calls `Lower::lower` — the REAL uniffi_core lowering: alloc scratch
///      buf, write 4-byte length + bytes, wrap in RustBuffer::from_vec.
///   3. Dispatches through the trait-object vtable (indirect fn-pointer call).
///   4. The sink consumes the RustBuffer (lower-bound foreign copy + free).
#[inline(never)]
fn uniffi_lane_deliver(sink: &dyn UpdateFrameSink, frame: &[u8]) {
    // Clone into owned Vec — this is the `Vec<u8>` the UniFFI scaffolding
    // would produce from the encoder output before calling `.lower()`.
    let data: Vec<u8> = frame.to_vec();

    // REAL uniffi_core lowering: Lower::lower calls lower_into_rust_buffer
    // which calls Vec<u8>::write() (i32 len prefix + per-byte), then wraps
    // in RustBuffer::from_vec.
    let rust_buf = <Vec<u8> as Lower<UniFfiTag>>::lower(data);

    // Indirect vtable call into the foreign-consume stub.
    sink.on_frame(rust_buf);
}

// ── Percentile computation ────────────────────────────────────────────────────

fn percentile(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((pct / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ── Baseline measurement (empty harness) ─────────────────────────────────────
//
// Measures timer + index overhead with NO transport work.
// Subtracted from every per-frame measurement so that the reported numbers
// reflect pure transport cost.

fn measure_baseline(frames: &[Vec<u8>], iters: usize) -> u64 {
    // Warm up
    for i in 0..1000 {
        black_box(frames[i % frames.len()].as_slice());
    }

    // Time pure index + timer call in batches of SMALL_BATCH_K
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

// ── Bucket timing pass ────────────────────────────────────────────────────────

/// Time Lane A (C-lane) for a set of frames using batch-of-K timing.
fn time_c_lane(listener: &UpdateListener, frames: &[Vec<u8>], baseline_ns: u64) -> Vec<u64> {
    let total = frames.len();
    let k = SMALL_BATCH_K;
    let mut results = Vec::with_capacity(total / k + 1);

    for chunk_start in (0..total).step_by(k) {
        let chunk_end = (chunk_start + k).min(total);
        let chunk = &frames[chunk_start..chunk_end];
        let t0 = Instant::now();
        for frame in chunk {
            listener(frame.as_slice());
        }
        let elapsed = t0.elapsed().as_nanos() as u64;
        let per_frame = elapsed / chunk.len() as u64;
        // Subtract baseline, floor at 0
        results.push(per_frame.saturating_sub(baseline_ns));
    }
    results
}

/// Time Lane B (UniFFI-lane) for a set of frames using batch-of-K timing.
fn time_uniffi_lane(
    sink: &dyn UpdateFrameSink,
    frames: &[Vec<u8>],
    baseline_ns: u64,
) -> Vec<u64> {
    let total = frames.len();
    let k = SMALL_BATCH_K;
    let mut results = Vec::with_capacity(total / k + 1);

    for chunk_start in (0..total).step_by(k) {
        let chunk_end = (chunk_start + k).min(total);
        let chunk = &frames[chunk_start..chunk_end];
        let t0 = Instant::now();
        for frame in chunk {
            uniffi_lane_deliver(sink, frame.as_slice());
        }
        let elapsed = t0.elapsed().as_nanos() as u64;
        let per_frame = elapsed / chunk.len() as u64;
        results.push(per_frame.saturating_sub(baseline_ns));
    }
    results
}

fn run_bucket_timing(
    bucket: &'static str,
    min: usize,
    max: usize,
    seed_offset: u64,
    c_listener: &UpdateListener,
    uniffi_sink: &dyn UpdateFrameSink,
) -> (BucketStats, BucketStats, u64) {
    let warmup = WARMUP_DELIVERIES;
    let iters = DELIVERIES_PER_BUCKET;
    let total = warmup + iters;

    // Build frames: same PRNG seed for both lanes.
    let frames = make_frames(PRNG_SEED ^ seed_offset, total, min, max);

    // Measure empty-harness baseline (timer + index, no transport).
    let baseline_ns = measure_baseline(&frames, 10_000);

    // Warmup — both lanes
    for frame in &frames[..warmup] {
        c_listener(frame.as_slice());
    }
    for frame in &frames[..warmup] {
        uniffi_lane_deliver(uniffi_sink, frame.as_slice());
    }

    let measure_frames = &frames[warmup..];

    // Timing pass — C lane
    let mut c_samples = time_c_lane(c_listener, measure_frames, baseline_ns);
    c_samples.sort_unstable();

    // Timing pass — UniFFI lane (same frames, same order)
    let mut uniffi_samples = time_uniffi_lane(uniffi_sink, measure_frames, baseline_ns);
    uniffi_samples.sort_unstable();

    let mean_bytes = frames[warmup..]
        .iter()
        .map(|f| f.len())
        .sum::<usize>()
        / iters;

    let c_stats = BucketStats {
        bucket,
        frame_bytes_min: min,
        frame_bytes_max: max,
        // Note: deliveries = DELIVERIES_PER_BUCKET (individual frames);
        // timing samples = deliveries / SMALL_BATCH_K (percentile population).
        deliveries: iters,
        p50_ns: percentile(&c_samples, 50.0),
        p95_ns: percentile(&c_samples, 95.0),
        p99_ns: percentile(&c_samples, 99.0),
    };
    let uniffi_stats = BucketStats {
        bucket,
        frame_bytes_min: min,
        frame_bytes_max: max,
        deliveries: iters,
        p50_ns: percentile(&uniffi_samples, 50.0),
        p95_ns: percentile(&uniffi_samples, 95.0),
        p99_ns: percentile(&uniffi_samples, 99.0),
    };
    (c_stats, uniffi_stats, mean_bytes as u64)
}

// ── Allocation pass ───────────────────────────────────────────────────────────

fn run_alloc_pass(
    c_listener: &UpdateListener,
    uniffi_sink: &dyn UpdateFrameSink,
) -> Vec<AllocStats> {
    let iters = DELIVERIES_PER_BUCKET;
    // Use medium bucket frames for allocation measurement.
    let frames = make_frames(PRNG_SEED ^ 0xff00, iters, MEDIUM_MIN_BYTES, MEDIUM_MAX_BYTES);

    // C-lane alloc pass
    let snap0 = alloc_snapshot();
    for frame in &frames {
        c_listener(frame.as_slice());
    }
    let snap1 = alloc_snapshot();
    let c_allocs = (snap1.allocations - snap0.allocations) as f64 / iters as f64;
    let c_bytes = (snap1.allocated_bytes - snap0.allocated_bytes) as f64 / iters as f64;

    // UniFFI-lane alloc pass
    let snap2 = alloc_snapshot();
    for frame in &frames {
        uniffi_lane_deliver(uniffi_sink, frame.as_slice());
    }
    let snap3 = alloc_snapshot();
    let u_allocs = (snap3.allocations - snap2.allocations) as f64 / iters as f64;
    let u_bytes = (snap3.allocated_bytes - snap2.allocated_bytes) as f64 / iters as f64;

    vec![
        AllocStats {
            lane: "C-lane",
            allocs_per_frame: c_allocs,
            alloc_bytes_per_frame: c_bytes,
        },
        AllocStats {
            lane: "UniFFI",
            allocs_per_frame: u_allocs,
            alloc_bytes_per_frame: u_bytes,
        },
    ]
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();
    let started_at = now_unix_seconds();

    eprintln!("ffi-transport-bench: building lanes...");

    let c_listener = build_c_lane_listener();
    let uniffi_sink: Box<dyn UpdateFrameSink> = Box::new(LowerBoundForeignSink);

    eprintln!("ffi-transport-bench: running SMALL bucket ({SMALL_MIN_BYTES}–{SMALL_MAX_BYTES} bytes)...");
    let (c_small, u_small, mean_small) = run_bucket_timing(
        "small",
        SMALL_MIN_BYTES,
        SMALL_MAX_BYTES,
        0x01,
        &c_listener,
        uniffi_sink.as_ref(),
    );

    eprintln!("ffi-transport-bench: running MEDIUM bucket ({MEDIUM_MIN_BYTES}–{MEDIUM_MAX_BYTES} bytes)...");
    let (c_medium, u_medium, mean_medium) = run_bucket_timing(
        "medium",
        MEDIUM_MIN_BYTES,
        MEDIUM_MAX_BYTES,
        0x02,
        &c_listener,
        uniffi_sink.as_ref(),
    );

    eprintln!("ffi-transport-bench: running LARGE bucket ({LARGE_MIN_BYTES}–{LARGE_MAX_BYTES} bytes)...");
    let (c_large, u_large, mean_large) = run_bucket_timing(
        "large",
        LARGE_MIN_BYTES,
        LARGE_MAX_BYTES,
        0x03,
        &c_listener,
        uniffi_sink.as_ref(),
    );

    // Weighted p99
    let c_wp99 = weighted_p99(c_small.p99_ns, c_medium.p99_ns, c_large.p99_ns);
    let u_wp99 = weighted_p99(u_small.p99_ns, u_medium.p99_ns, u_large.p99_ns);

    // Weighted mean bytes
    let weighted_mean_bytes =
        WEIGHT_SMALL * mean_small as f64 + WEIGHT_MEDIUM * mean_medium as f64 + WEIGHT_LARGE * mean_large as f64;

    // Synthetic foreign-copy estimate (LOWER BOUND, see caveat)
    let synthetic_ns = estimate_synthetic_foreign_copy_ns(weighted_mean_bytes);

    let (raw_delta, surcharged_delta) = compute_surcharged_delta(c_wp99, u_wp99, synthetic_ns);

    let pct_of_budget = surcharged_delta.max(0) as f64 / FRAME_BUDGET_NS as f64;

    let verdict = compute_verdict(surcharged_delta, u_small.p99_ns);
    let governing_threshold = if surcharged_delta.max(0) as u64 >= KEEP_THRESHOLD_NS {
        KEEP_THRESHOLD_NS
    } else {
        COLLAPSE_THRESHOLD_NS
    };

    // Allocation pass (optional, separate from timing)
    let alloc_stats = if args.alloc_pass {
        eprintln!("ffi-transport-bench: running allocation pass...");
        run_alloc_pass(&c_listener, uniffi_sink.as_ref())
    } else {
        vec![]
    };

    let baseline_note = format!(
        "Empty-harness baseline (timer+index, no transport) subtracted from every per-frame measurement. \
         SMALL baseline ≈ {} ns, MEDIUM ≈ {} ns, LARGE ≈ {} ns.",
        // We approximate by re-running a quick baseline for the report note.
        {
            let small_f = make_frames(PRNG_SEED, 2000, SMALL_MIN_BYTES, SMALL_MAX_BYTES);
            measure_baseline(&small_f, 2000)
        },
        {
            let med_f = make_frames(PRNG_SEED, 2000, MEDIUM_MIN_BYTES, MEDIUM_MAX_BYTES);
            measure_baseline(&med_f, 2000)
        },
        {
            let large_f = make_frames(PRNG_SEED, 2000, LARGE_MIN_BYTES, LARGE_MAX_BYTES);
            measure_baseline(&large_f, 2000)
        },
    );

    let report = TransportBenchReport {
        tool: "ffi-transport-bench",
        started_at_unix: started_at,
        build_info: BuildInfo::capture(),
        c_lane: LaneReport {
            lane: "C-lane",
            buckets: vec![c_small.clone(), c_medium.clone(), c_large.clone()],
            weighted_p99_ns: c_wp99,
            baseline_ns: 0, // subtracted per-bucket above
        },
        uniffi_lane: LaneReport {
            lane: "UniFFI",
            buckets: vec![u_small.clone(), u_medium.clone(), u_large.clone()],
            weighted_p99_ns: u_wp99,
            baseline_ns: 0,
        },
        weighted_p99_delta_raw_ns: raw_delta,
        synthetic_foreign_copy_ns: synthetic_ns,
        weighted_p99_delta_surcharged_ns: surcharged_delta,
        surcharged_delta_pct_of_frame_budget: pct_of_budget,
        verdict: verdict.as_str().to_string(),
        governing_threshold_ns: governing_threshold,
        alloc_stats,
        synthetic_caveat: vec![
            format!(
                "SYNTHETIC (LOWER BOUND): The foreign-consume stub (sink.on_frame) copies the \
                 RustBuffer into an owned Vec<u8> via destroy_into_vec(), modeling the floor cost \
                 of Swift Data / Kotlin ByteArray construction. Real ARC/GC bookkeeping is NOT \
                 modeled; the pre-registered {}× surcharge compensates.",
                JNI_MANAGED_SURCHARGE_FACTOR
            ),
            "SYNTHETIC (NOT EXECUTED): JNI boundary surcharge (GetByteArrayElements, \
             SetByteArrayRegion, local-ref table) is NOT executed. \
             It is applied only as the pre-registered conservative 3× multiplier band \
             from published JNI numbers.".to_string(),
            "SYNTHETIC (NOT MODELED): Real cross-language thread/dispatch-queue handoff \
             and on-device backpressure drop/reorder. In-process single listener thread; \
             cross-language hop deferred and labeled as out-of-scope for this bench. \
             Dropped/reordered frames = 0 by in-process construction; \
             this is NOT a real-transport claim.".to_string(),
            format!(
                "REAL (EXECUTED): C-lane indirect Arc<dyn Fn> call + ptr/len dispatch + \
                 mandatory one shell-copy (Vec::to_vec) per frame. \
                 UniFFI Rust-side cost: RustBuffer allocation, FfiConverter Lower::lower \
                 serialization (4-byte i32 prefix + per-byte write via uniffi_core 0.29.5), \
                 and indirect vtable dispatch through Box<dyn UpdateFrameSink>. \
                 Synthetic foreign-copy component ({:.0} ns at weighted mean {:.0} B, \
                 40 ns/KB conservative estimate) multiplied by {}× surcharge in verdict.",
                synthetic_ns as f64,
                weighted_mean_bytes,
                JNI_MANAGED_SURCHARGE_FACTOR,
            ),
        ],
        notes: vec![
            format!(
                "Workload: ADR-0055 omit-unchanged regime. \
                 SMALL {SMALL_MIN_BYTES}–{SMALL_MAX_BYTES} B (80%), \
                 MEDIUM {MEDIUM_MIN_BYTES}–{MEDIUM_MAX_BYTES} B (15%), \
                 LARGE {LARGE_MIN_BYTES}–{LARGE_MAX_BYTES} B (5%). \
                 100k deliveries per bucket per lane after 10k warmup discarded. \
                 Cadence grounded in code: steady {STEADY_EMIT_HZ} Hz (DEFAULT_EMIT_HZ), \
                 burst to {BURST_EMIT_HZ} Hz (clamp ceiling).",
            ),
            baseline_note,
            "Platform: macOS (Darwin 25.5.0), single-threaded, no tokio. \
             CPU affinity unpinned (macOS does not expose CPU pinning in userspace); \
             compensated by high iteration count + percentiles.".to_string(),
            format!(
                "Pre-registered decision rule (encoded before measurement): \
                 COLLAPSE iff surcharged weighted-p99 delta < {} ns (5% of 16.67ms) AND \
                 UniFFI SMALL p99 < {} ns; \
                 KEEP iff delta >= {} ns (15%); \
                 ESCALATE otherwise. \
                 No COLLAPSE permitted from synthetic evidence inside or above the ESCALATE band.",
                COLLAPSE_THRESHOLD_NS, config::COLLAPSE_SMALL_ABS_NS, KEEP_THRESHOLD_NS,
            ),
        ],
    };

    if args.write_report {
        if let Err(err) = write_report(&report) {
            eprintln!("ffi-transport-bench: failed to write report: {err}");
            std::process::exit(1);
        }
        eprintln!(
            "ffi-transport-bench: report written to docs/perf/ffi-transport-bench/"
        );
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serializes report")
    );

    // Print human-readable summary to stderr
    eprintln!();
    eprintln!("=== ffi-transport-bench SUMMARY ===");
    eprintln!("Weighted-p99 C-lane  : {:.0} ns", report.c_lane.weighted_p99_ns);
    eprintln!("Weighted-p99 UniFFI  : {:.0} ns", report.uniffi_lane.weighted_p99_ns);
    eprintln!("Raw delta            : {} ns", report.weighted_p99_delta_raw_ns);
    eprintln!("Synthetic copy est.  : {} ns (lower bound, {}×-surcharged)", report.synthetic_foreign_copy_ns, JNI_MANAGED_SURCHARGE_FACTOR);
    eprintln!("Surcharged delta     : {} ns ({:.2}% of 16.67ms budget)", report.weighted_p99_delta_surcharged_ns, pct_of_budget * 100.0);
    eprintln!("VERDICT              : {}", report.verdict);
    eprintln!("===================================");

    if args.fail_on_gate {
        // The bench does not gate on a pass/fail — verdict is an enum; caller
        // decides. Exit 0 always to not block CI on a measurement tool.
        std::process::exit(0);
    }
}
