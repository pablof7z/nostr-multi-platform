//! sanity-gate — ABSOLUTE new-architecture sanity gates.
//!
//! GOAL: detect whether the CURRENT NMP architecture MISBEHAVES under real load
//! — CPU pegging / busy-spin / polling, memory leaks (unbounded RSS), latency
//! cliffs, dropped events, or correctness breaks. Absolute thresholds, not
//! deltas. This is the in-process half of the harness; the OS-metric half (CPU%,
//! per-thread CPU, RSS-over-time) is captured by `scripts/perf-sanity/` and
//! merged via `--os-metrics`.
//!
//! Grounding (file:line):
//! - docs/aim.md §1 (l.11/l.15 thin-shell + one-shot-app), §2 invariant 9
//!   (no high-frequency FFI loops; idle must be quiet — the no-spin gate).
//! - AGENTS.md l.104 (500-LOC hard ceiling — this bin is split into modules).
//! - firehose-bench/config.rs:1-5 (latency + memory gate constants — REUSED).
//! - firehose-bench/live/cold_start.rs (live-driver pattern — adapted, not edited).
//! - ffi-stress/ffi.rs (mach RSS) + s7_feed_idle.rs (capture_cb ctx pattern — reused).
//! - reactivity-bench/allocator.rs (CountingAllocator — pattern copied).
//! - real_relay_common/mod.rs (SKIP-LOUD honest-validation — mirrored as
//!   Verdict::SkipRelayMiss).
//! - kernel/ram_eviction.rs (LRU/HWM — referenced by the memory gate's
//!   documented hook gap).

mod config;
mod driver;
mod metrics;
mod oracle;
mod phases;
mod report;

use config::{Args, Phase};
use report::{SanityReport, Verdict};

fn main() {
    let args = Args::parse();
    let mode: &'static str = if args.live { "live" } else { "local" };

    let mut report = SanityReport::new(mode, args.run_id.clone(), args.relay.clone());

    for phase in args.phase.expand() {
        eprintln!("sanity-gate: running phase `{}` …", phase.as_str());
        match phase {
            Phase::ColdStart => phases::run_cold_start(&mut report, &args),
            Phase::IdleSoak => phases::run_idle_soak(&mut report, &args),
            Phase::MemorySoak => phases::run_memory_soak(&mut report, &args),
            Phase::Firehose => phases::run_firehose(&mut report, &args),
            Phase::Correctness => oracle::run_correctness(&mut report, &args),
            Phase::Reactive => phases::run_reactive(&mut report, &args),
            Phase::Resilience => phases::run_resilience(&mut report, &args),
            Phase::Privacy => phases::run_privacy(&mut report, &args),
            Phase::FfiBounds => phases::run_ffi_bounds(&mut report, &args),
            Phase::GcSoundness => phases::run_gc_soundness(&mut report, &args),
            Phase::All => unreachable!("expand() removes All"),
        }
    }

    emit_standing_findings(&mut report);

    let path = match report.write() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("sanity-gate: failed to write report: {e}");
            std::process::exit(1);
        }
    };

    println!("{}", serde_json::to_string_pretty(&report).expect("serialize"));
    eprintln!("sanity-gate: report written to {}", path.display());
    summarize(&report);

    if args.fail_on_gate && !report.overall_passed {
        std::process::exit(2);
    }
}

fn summarize(report: &SanityReport) {
    let mut pass = 0;
    let mut fail = 0;
    let mut skip = 0;
    let mut blocked = 0;
    for r in &report.rows {
        match r.verdict {
            Verdict::Pass => pass += 1,
            Verdict::Fail => fail += 1,
            Verdict::SkipRelayMiss => skip += 1,
            Verdict::Blocked => blocked += 1,
        }
    }
    eprintln!(
        "sanity-gate: {pass} PASS / {fail} FAIL / {skip} SKIP-relay-miss / {blocked} BLOCKED \
         (overall_passed={})",
        report.overall_passed
    );
}

/// Findings that always apply regardless of which phases ran: the documented
/// hook gaps and the iOS/Android capture stubs left for later.
fn emit_standing_findings(report: &mut SanityReport) {
    report.finding(
        "HOOK GAP — per-event ingest→emit p99: the per-event ingest-timestamp counter is \
         #[cfg(test)] inside the store and not on the typed wire. The firehose phase reports an \
         AMORTISED inject→visible cost instead. Follow-up: add a process-lifetime ingest-latency \
         histogram read hook (sibling to nmp_app_read_projection_churn_stats) — do NOT modify the \
         churning store.",
    );
    report.finding(
        "HOOK GAP — LRU-eviction evidence: kernel::ram_eviction::RamEvictionReport is internal to \
         run_gc_step with no FFI/diagnostics read seam. The memory phase marks the \
         lru-evictions-occur gate BLOCKED. Follow-up: nmp_app_read_ram_eviction_stats counter.",
    );
    report.finding(
        "HOOK GAP — follow-feed author subset + replaceable supersession: the typed \
         SnapshotEnvelope exposes visible_items (count) but not per-item author hex, nor the \
         resolved replaceable value. The set-inclusion and newest-wins oracles are BLOCKED with \
         the exact seams to add (nmp_app_read_feed_authors / nmp_app_read_replaceable).",
    );
    report.finding(
        "HOOK GAP — query_visit conversion counter is #[cfg(test)] (issue #1522). Not wired; \
         measured nothing here per the brief's instruction not to touch the churning store code.",
    );
    report.finding(
        "STUB (iOS) — Instruments/xctrace capture path: drive the Chirp iOS shell under \
         `xcrun xctrace record --template 'Time Profiler' --template 'Allocations'` (or via the \
         xcode MCP) for idle-CPU + per-thread spin + RSS. Map xctrace's per-thread sample %, \
         persistent-bytes, and CPU-usage tracks onto the same gate names (idle-cpu, \
         no-spin-per-thread, memory-slope). See scripts/perf-sanity/README.md §iOS.",
    );
    report.finding(
        "STUB (Android) — dumpsys/perfetto capture path: `adb shell dumpsys cpuinfo`/`meminfo` for \
         coarse CPU%/RSS, and a perfetto trace (`record_android_trace`) for per-thread scheduling \
         to feed the no-spin gate. Map onto the same gate names. See scripts/perf-sanity/README.md \
         §Android.",
    );
    report.finding(
        "BLOCKED on unmerged work — #1552 (dispatcher), #1541 (wakeups), and the pull cursor: the \
         idle-cpu and no-spin gates are the exact detectors for a wakeup-storm/poll regression \
         those changes touch. Re-run --phase idle_soak once they land; this harness already has \
         the scenario wired (it just needs the OS sidecar numbers).",
    );
    report.finding(
        "Run the FULL harness (nak serve + OS sidecar + sign-in-as-account) via \
         `scripts/perf-sanity/run.sh`; this Rust bin alone covers the in-process gates and emits \
         BLOCKED rows for anything needing the OS sidecar or a real high-follow fixture.",
    );
}
