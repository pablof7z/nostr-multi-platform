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
//   - The foreign-consume stub (memcpy RustBuffer -> owned Vec + free) is a
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
mod bench_timing;
mod config;
mod frames;
mod lanes;
mod report;
mod rng;

use allocator::alloc_snapshot;
use bench_timing::{measure_baseline, run_bucket_timing};
use config::{
    Args, BURST_EMIT_HZ, COLLAPSE_THRESHOLD_NS, DELIVERIES_PER_BUCKET, FRAME_BUDGET_NS,
    JNI_MANAGED_SURCHARGE_FACTOR, KEEP_THRESHOLD_NS, LARGE_MAX_BYTES, LARGE_MIN_BYTES,
    MEDIUM_MAX_BYTES, MEDIUM_MIN_BYTES, PRNG_SEED, SMALL_MAX_BYTES, SMALL_MIN_BYTES,
    STEADY_EMIT_HZ, WEIGHT_LARGE, WEIGHT_MEDIUM, WEIGHT_SMALL,
};
use frames::make_frames;
use lanes::{build_c_lane_listener, uniffi_lane_deliver, LowerBoundForeignSink, UpdateFrameSink};
use report::{
    compute_surcharged_delta, compute_verdict, estimate_synthetic_foreign_copy_ns,
    now_unix_seconds, weighted_p99, write_report, AllocStats, BuildInfo, LaneReport,
    TransportBenchReport, Verdict,
};

// ── Allocation pass ───────────────────────────────────────────────────────────

fn run_alloc_pass(
    c_listener: &dyn Fn(&[u8]),
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

    eprintln!("ffi-transport-bench: running SMALL bucket ({SMALL_MIN_BYTES}-{SMALL_MAX_BYTES} bytes)...");
    // SMALL: batch-mean only (sub-us C-lane cost, near Instant timer resolution).
    let (c_small, u_small, mean_small) = run_bucket_timing(
        "small",
        SMALL_MIN_BYTES,
        SMALL_MAX_BYTES,
        0x01,
        false, // per_frame_p99 = false (sub-us, below timer resolution)
        &c_listener,
        uniffi_sink.as_ref(),
    );

    eprintln!("ffi-transport-bench: running MEDIUM bucket ({MEDIUM_MIN_BYTES}-{MEDIUM_MAX_BYTES} bytes)...");
    // MEDIUM: batch-mean + per-frame (hundreds of ns, above timer resolution).
    let (c_medium, u_medium, mean_medium) = run_bucket_timing(
        "medium",
        MEDIUM_MIN_BYTES,
        MEDIUM_MAX_BYTES,
        0x02,
        true, // per_frame_p99 = true (feasible above timer resolution)
        &c_listener,
        uniffi_sink.as_ref(),
    );

    eprintln!("ffi-transport-bench: running LARGE bucket ({LARGE_MIN_BYTES}-{LARGE_MAX_BYTES} bytes)...");
    // LARGE: batch-mean + per-frame (us range, clearly above timer resolution).
    let (c_large, u_large, mean_large) = run_bucket_timing(
        "large",
        LARGE_MIN_BYTES,
        LARGE_MAX_BYTES,
        0x03,
        true, // per_frame_p99 = true (feasible)
        &c_listener,
        uniffi_sink.as_ref(),
    );

    // Weighted p99 (batch-mean — see METRIC LABELING NOTE in report.rs).
    let c_wp99 = weighted_p99(
        c_small.p99_batch_mean_ns,
        c_medium.p99_batch_mean_ns,
        c_large.p99_batch_mean_ns,
    );
    let u_wp99 = weighted_p99(
        u_small.p99_batch_mean_ns,
        u_medium.p99_batch_mean_ns,
        u_large.p99_batch_mean_ns,
    );

    // Weighted mean bytes
    let weighted_mean_bytes = WEIGHT_SMALL * mean_small as f64
        + WEIGHT_MEDIUM * mean_medium as f64
        + WEIGHT_LARGE * mean_large as f64;

    // Synthetic foreign-copy estimate (LOWER BOUND, see caveat).
    let synthetic_ns = estimate_synthetic_foreign_copy_ns(weighted_mean_bytes);

    let (raw_delta, surcharged_delta) = compute_surcharged_delta(c_wp99, u_wp99, synthetic_ns);

    let pct_of_budget = surcharged_delta.max(0) as f64 / FRAME_BUDGET_NS as f64;

    // The verdict uses the SMALL-bucket batch-mean p99 for the absolute gate.
    // This is labeled explicitly: COLLAPSE_SMALL_ABS_NS checks batch-mean p99.
    let verdict_enum = compute_verdict(surcharged_delta, u_small.p99_batch_mean_ns);
    let governing_threshold = if surcharged_delta.max(0) as u64 >= KEEP_THRESHOLD_NS {
        KEEP_THRESHOLD_NS
    } else {
        COLLAPSE_THRESHOLD_NS
    };

    // Allocation pass (optional, separate from timing).
    let alloc_stats = if args.alloc_pass {
        eprintln!("ffi-transport-bench: running allocation pass...");
        run_alloc_pass(c_listener.as_ref(), uniffi_sink.as_ref())
    } else {
        vec![]
    };

    let baseline_note = format!(
        "Empty-harness baseline (timer+index, no transport) subtracted from every per-frame measurement. \
         SMALL baseline ~{} ns, MEDIUM ~{} ns, LARGE ~{} ns.",
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
        verdict: verdict_enum.as_str().to_string(),
        verdict_caveat: format!(
            "COLLAPSE assumes real managed/JNI per-frame overhead delta cannot reach \
             ~{} us; that boundary is modeled ({}x surcharge on synthetic foreign-copy \
             lower bound), not measured. On-device A/B is the escalation if ever doubted.",
            COLLAPSE_THRESHOLD_NS / 1_000,
            JNI_MANAGED_SURCHARGE_FACTOR,
        ),
        governing_threshold_ns: governing_threshold,
        alloc_stats,
        synthetic_caveat: vec![
            format!(
                "SYNTHETIC (LOWER BOUND): The foreign-consume stub (sink.on_frame) copies the \
                 RustBuffer into an owned Vec<u8> via destroy_into_vec(), modeling the floor cost \
                 of Swift Data / Kotlin ByteArray construction. Real ARC/GC bookkeeping is NOT \
                 modeled; the pre-registered {}x surcharge compensates.",
                JNI_MANAGED_SURCHARGE_FACTOR
            ),
            "SYNTHETIC (NOT EXECUTED): JNI boundary surcharge (GetByteArrayElements, \
             SetByteArrayRegion, local-ref table) is NOT executed. \
             It is applied only as the pre-registered conservative 3x multiplier band \
             from published JNI numbers."
                .to_string(),
            "SYNTHETIC (NOT MODELED): Real cross-language thread/dispatch-queue handoff \
             and on-device backpressure drop/reorder. In-process single listener thread; \
             cross-language hop deferred and labeled as out-of-scope for this bench. \
             Dropped/reordered frames = 0 by in-process construction; \
             this is NOT a real-transport claim."
                .to_string(),
            format!(
                "REAL (EXECUTED): C-lane indirect Arc<dyn Fn> call + ptr/len dispatch + \
                 mandatory one shell-copy (Vec::to_vec) per frame. \
                 UniFFI Rust-side cost: RustBuffer allocation, FfiConverter Lower::lower \
                 serialization (4-byte i32 prefix + per-byte write via uniffi_core 0.29.5), \
                 and indirect vtable dispatch through Box<dyn UpdateFrameSink>. \
                 Synthetic foreign-copy component ({:.0} ns at weighted mean {:.0} B, \
                 40 ns/KB conservative estimate) multiplied by {}x surcharge in verdict.",
                synthetic_ns as f64,
                weighted_mean_bytes,
                JNI_MANAGED_SURCHARGE_FACTOR,
            ),
        ],
        notes: vec![
            format!(
                "Workload: ADR-0055 omit-unchanged regime. \
                 SMALL {SMALL_MIN_BYTES}-{SMALL_MAX_BYTES} B (80%), \
                 MEDIUM {MEDIUM_MIN_BYTES}-{MEDIUM_MAX_BYTES} B (15%), \
                 LARGE {LARGE_MIN_BYTES}-{LARGE_MAX_BYTES} B (5%). \
                 100k deliveries per bucket per lane after 10k warmup discarded. \
                 Cadence grounded in code: steady {STEADY_EMIT_HZ} Hz (DEFAULT_EMIT_HZ), \
                 burst to {BURST_EMIT_HZ} Hz (clamp ceiling).",
            ),
            baseline_note,
            "Platform: macOS (Darwin 25.5.0), single-threaded, no tokio. \
             CPU affinity unpinned (macOS does not expose CPU pinning in userspace); \
             compensated by high iteration count + percentiles."
                .to_string(),
            format!(
                "Pre-registered decision rule (encoded before measurement): \
                 COLLAPSE iff surcharged weighted-p99 delta < {} ns (5% of 16.67ms) AND \
                 UniFFI SMALL batch-mean p99 < {} ns; \
                 KEEP iff delta >= {} ns (15%); \
                 ESCALATE otherwise. \
                 No COLLAPSE permitted from synthetic evidence inside or above the ESCALATE band.",
                COLLAPSE_THRESHOLD_NS,
                config::COLLAPSE_SMALL_ABS_NS,
                KEEP_THRESHOLD_NS,
            ),
        ],
    };

    if args.write_report {
        if let Err(err) = write_report(&report) {
            eprintln!("ffi-transport-bench: failed to write report: {err}");
            std::process::exit(1);
        }
        eprintln!("ffi-transport-bench: report written to docs/perf/ffi-transport-bench/");
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serializes report")
    );

    // Print human-readable summary to stderr
    eprintln!();
    eprintln!("=== ffi-transport-bench SUMMARY ===");
    eprintln!(
        "Weighted-p99 C-lane (batch-mean) : {:.0} ns",
        report.c_lane.weighted_p99_ns
    );
    eprintln!(
        "Weighted-p99 UniFFI (batch-mean) : {:.0} ns",
        report.uniffi_lane.weighted_p99_ns
    );
    eprintln!("Raw delta                        : {} ns", report.weighted_p99_delta_raw_ns);
    eprintln!(
        "Synthetic copy est.              : {} ns (lower bound, {}x-surcharged)",
        report.synthetic_foreign_copy_ns, JNI_MANAGED_SURCHARGE_FACTOR
    );
    eprintln!(
        "Surcharged delta                 : {} ns ({:.2}% of 16.67ms budget)",
        report.weighted_p99_delta_surcharged_ns,
        pct_of_budget * 100.0
    );
    eprintln!("VERDICT                          : {}", report.verdict);
    if let Some(v) = c_medium.p99_per_frame_ns {
        eprintln!("C-lane MEDIUM p99/frame          : {} ns", v);
    }
    if let Some(v) = u_medium.p99_per_frame_ns {
        eprintln!("UniFFI MEDIUM p99/frame          : {} ns", v);
    }
    if let Some(v) = c_large.p99_per_frame_ns {
        eprintln!("C-lane LARGE  p99/frame          : {} ns", v);
    }
    if let Some(v) = u_large.p99_per_frame_ns {
        eprintln!("UniFFI LARGE  p99/frame          : {} ns", v);
    }
    eprintln!("===================================");

    if args.fail_on_gate {
        // CI gate: exit non-zero if the verdict is not COLLAPSE.
        match verdict_enum {
            Verdict::Collapse => std::process::exit(0),
            _ => {
                eprintln!(
                    "ffi-transport-bench: CI GATE FAIL -- verdict {} is not COLLAPSE. \
                     Re-run on device or review thresholds.",
                    report.verdict
                );
                std::process::exit(1);
            }
        }
    }
}
