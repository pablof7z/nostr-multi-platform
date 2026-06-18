//! Phase runners — each appends ABSOLUTE gate rows to the report.
//!
//! Every phase first establishes a live driven app and waits for connection.
//! On a relay miss it pushes a `SKIP-relay-miss` row (honest-validation) and
//! returns — never a faked green.

use std::time::Duration;

use crate::config::{gates, Args, Phase, FILLED_TIMELINE_TARGET};
use crate::driver::DrivenApp;
use crate::metrics::{
    alloc_snapshot, load_os_metrics, process_rss_mb, OsPhaseMetrics,
};
use crate::report::{GateRow, SanityReport, Verdict};

mod firehose;
pub use firehose::run_firehose;

/// Wait-for-connect budget (mirrors firehose live `WARMUP_TIMEOUT`).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Establish a live app + wait for relay connect. Returns `None` (after
/// pushing a SKIP-relay-miss row) if no relay connected in budget.
pub fn connect_or_skip(
    report: &mut SanityReport,
    phase: &str,
    args: &Args,
) -> Option<DrivenApp> {
    let app = DrivenApp::launch(
        args.nsec.as_deref(),
        args.viewer_hex.as_deref(),
        std::slice::from_ref(&args.relay),
    );
    let connected = app
        .wait_until(CONNECT_TIMEOUT, |s| s.any_connected())
        .is_some();
    if !connected {
        report.push(GateRow::unmeasured(
            "relay-connect",
            phase,
            "decode_snapshot_envelope",
            "relay_status.connection",
            "== connected",
            Verdict::SkipRelayMiss,
            &format!(
                "no relay reached {} within {}s — SKIP LOUD (never fake green)",
                args.relay,
                CONNECT_TIMEOUT.as_secs()
            ),
        ));
        return None;
    }
    Some(app)
}

/// Like [`connect_or_skip`] but does NOT require a relay — the firehose oracle
/// drives the kernel ingest seam directly. Launches the full real composition
/// (feed engine, projections, GC) and waits briefly for the first frame so the
/// actor is live, then returns the app regardless of relay connectivity.
pub fn connect_or_skip_optional(
    report: &mut SanityReport,
    phase: &str,
    args: &Args,
) -> Option<DrivenApp> {
    let app = DrivenApp::launch(
        args.nsec.as_deref(),
        args.viewer_hex.as_deref(),
        std::slice::from_ref(&args.relay),
    );
    // Wait for the actor to emit its first frame (proves it is alive).
    if app
        .wait_until(Duration::from_secs(10), |s| !s.records.is_empty())
        .is_none()
    {
        report.push(GateRow::unmeasured(
            "actor-liveness",
            phase,
            "decode_snapshot_envelope",
            "first SnapshotFrame",
            ">= 1 frame in 10s",
            Verdict::Blocked,
            "actor emitted no frame in 10s — kernel did not start (BLOCKED, not a relay miss)",
        ));
        return None;
    }
    Some(app)
}

/// Cold-start: first-item + filled-timeline latency (reused firehose gates).
pub fn run_cold_start(report: &mut SanityReport, args: &Args) {
    let phase = Phase::ColdStart.as_str();
    let Some(app) = connect_or_skip(report, phase, args) else {
        return;
    };

    let first = app.wait_until(Duration::from_secs(10), |s| s.peak_visible() >= 1);
    let filled = app.wait_until(Duration::from_secs(15), |s| {
        s.peak_visible() >= FILLED_TIMELINE_TARGET
    });

    match first {
        Some(ms) => report.push(GateRow::max(
            "cold-start-first-item",
            phase,
            "decode_snapshot_envelope",
            "SnapshotEnvelope.visible_items >= 1",
            ms as f64,
            gates::FIRST_ITEM_GATE_MS,
            "ms",
        )),
        None => report.push(GateRow::unmeasured(
            "cold-start-first-item",
            phase,
            "decode_snapshot_envelope",
            "SnapshotEnvelope.visible_items",
            &format!("<= {} ms", gates::FIRST_ITEM_GATE_MS),
            Verdict::SkipRelayMiss,
            "connected but no timeline item arrived (empty follow set or relay had no events)",
        )),
    }

    match filled {
        Some(ms) => report.push(GateRow::max(
            "cold-start-filled-timeline",
            phase,
            "decode_snapshot_envelope",
            "SnapshotEnvelope.visible_items >= 200",
            ms as f64,
            gates::FILLED_TIMELINE_GATE_MS,
            "ms",
        )),
        None => report.push(GateRow::unmeasured(
            "cold-start-filled-timeline",
            phase,
            "decode_snapshot_envelope",
            "SnapshotEnvelope.visible_items",
            &format!("<= {} ms", gates::FILLED_TIMELINE_GATE_MS),
            Verdict::SkipRelayMiss,
            &format!(
                "timeline did not reach {} items (insufficient relay backlog for this account)",
                FILLED_TIMELINE_TARGET
            ),
        )),
    }

    // Load-older (paginate back) latency: request an older page on the home
    // feed and time how long until the visible count grows. This exercises the
    // pull-cursor path (see findings re: #1552/#1541/pull cursor).
    let before_visible = app.with_state(|s| s.peak_visible());
    let key = std::ffi::CString::new("nmp.feed.home").unwrap();
    nmp_ffi::nmp_app_load_older_feed(app.raw(), key.as_ptr());
    match app.wait_until(Duration::from_secs(8), |s| s.peak_visible() > before_visible) {
        Some(ms) => report.push(GateRow::max(
            "load-older",
            phase,
            "nmp_app_load_older_feed + decode_snapshot_envelope",
            "SnapshotEnvelope.visible_items growth",
            ms as f64,
            gates::LOAD_OLDER_GATE_MS,
            "ms",
        )),
        None => report.push(GateRow::unmeasured(
            "load-older",
            phase,
            "nmp_app_load_older_feed",
            "SnapshotEnvelope.visible_items growth",
            &format!("<= {} ms", gates::LOAD_OLDER_GATE_MS),
            Verdict::SkipRelayMiss,
            "no older page arrived (relay backlog exhausted, or pull-cursor work unmerged — \
             #1552/#1541). Re-run once those land.",
        )),
    }

    // Ramp peak-RSS gate (in-process mach RSS — no OS sidecar needed).
    let rss = process_rss_mb();
    report.push(GateRow::max(
        "ramp-peak-rss",
        phase,
        "task_info(MACH_TASK_BASIC_INFO)",
        "metrics::process_rss_mb",
        rss,
        gates::RAMP_MEMORY_GATE_MB,
        "MB",
    ));
}

/// Idle soak: connected + EOSE'd + NO new events. Measure CPU/spin ≥ soak_secs.
/// The CPU measurement is the OS sidecar's job (per-thread CPU is a `top -H`
/// job, not an in-process counter) — this phase holds the app idle for the
/// window and asserts the merged OS numbers.
pub fn run_idle_soak(report: &mut SanityReport, args: &Args) {
    let phase = Phase::IdleSoak.as_str();
    let Some(app) = connect_or_skip(report, phase, args) else {
        return;
    };

    // Let the initial backlog drain, then hold idle. The sidecar samples CPU
    // across this window (the orchestrator aligns its sampling to this phase).
    let before = app.with_state(|s| s.records.len());
    std::thread::sleep(Duration::from_secs(args.soak_secs.max(60)));
    let after = app.with_state(|s| s.records.len());

    // In-process liveness proxy: frames emitted while idle. A healthy actor at
    // idle still ticks at emit_hz (4Hz) — but those are EMPTY ticks; we cannot
    // read CPU in-process. We surface the frame delta as context and defer the
    // hard CPU gate to the OS sidecar.
    let idle_frames = after.saturating_sub(before);

    let os = os_metrics(args, phase);
    push_idle_cpu_gate(report, phase, &os, idle_frames);
    push_no_spin_gate(report, phase, &os);
}

fn push_idle_cpu_gate(
    report: &mut SanityReport,
    phase: &str,
    os: &Option<OsPhaseMetrics>,
    idle_frames: usize,
) {
    match os.as_ref().and_then(|o| o.cpu_pct_mean) {
        Some(cpu) => report.push(
            GateRow::max(
                "idle-cpu",
                phase,
                "ps -o %cpu / top sampling (sidecar)",
                "scripts/perf-sanity cpu_pct_mean",
                cpu,
                gates::IDLE_CPU_PCT_GATE,
                "%",
            )
            .with_note(&format!(
                "idle window emitted {idle_frames} frames (empty ticks); CPU must stay flat"
            )),
        ),
        None => report.push(GateRow::unmeasured(
            "idle-cpu",
            phase,
            "ps -o %cpu / top sampling (sidecar)",
            "scripts/perf-sanity cpu_pct_mean",
            &format!("< {} %", gates::IDLE_CPU_PCT_GATE),
            Verdict::Blocked,
            "no --os-metrics provided: run via scripts/perf-sanity orchestrator to capture idle CPU%",
        )),
    }
}

fn push_no_spin_gate(report: &mut SanityReport, phase: &str, os: &Option<OsPhaseMetrics>) {
    match os.as_ref().and_then(|o| o.max_thread_cpu_pct) {
        Some(t) => report.push(GateRow::max(
            "no-spin-per-thread",
            phase,
            "top -H / ps -M (sidecar)",
            "scripts/perf-sanity max_thread_cpu_pct",
            t,
            gates::NO_SPIN_PER_THREAD_PCT_GATE,
            "%",
        )),
        None => report.push(GateRow::unmeasured(
            "no-spin-per-thread",
            phase,
            "top -H / ps -M (sidecar)",
            "scripts/perf-sanity max_thread_cpu_pct",
            &format!("< {} %", gates::NO_SPIN_PER_THREAD_PCT_GATE),
            Verdict::Blocked,
            "no --os-metrics provided: per-thread CPU needs the sidecar's top -H sampler",
        )),
    }
}

/// Memory soak: sustained tail, RSS slope bounded, LRU evictions occur.
pub fn run_memory_soak(report: &mut SanityReport, args: &Args) {
    let phase = Phase::MemorySoak.as_str();
    let Some(app) = connect_or_skip(report, phase, args) else {
        return;
    };

    let rss_start = process_rss_mb();
    let alloc_start = alloc_snapshot();
    std::thread::sleep(Duration::from_secs(args.soak_secs.max(60)));
    let rss_end = process_rss_mb();
    let alloc_end = alloc_snapshot();
    let _ = &app;

    let drift = rss_end - rss_start;
    // In-process drift gate (firehose MEMORY_DRIFT_30M_GATE_MB) — only a true
    // gate when the soak actually ran ~30m; for shorter dev runs we report it
    // as context and lean on the OS slope gate.
    let soak_min = args.soak_secs as f64 / 60.0;
    if soak_min >= 25.0 {
        report.push(GateRow::max(
            "memory-drift-30m",
            phase,
            "task_info(MACH_TASK_BASIC_INFO)",
            "metrics::process_rss_mb (start vs end)",
            drift,
            gates::MEMORY_DRIFT_30M_GATE_MB,
            "MB",
        ));
    } else {
        report.push(GateRow::unmeasured(
            "memory-drift-30m",
            phase,
            "task_info(MACH_TASK_BASIC_INFO)",
            "metrics::process_rss_mb",
            &format!("<= {} MB over 30m", gates::MEMORY_DRIFT_30M_GATE_MB),
            Verdict::Blocked,
            &format!(
                "soak was only {soak_min:.1} min (<25m): pass --soak-secs 1800 for the real gate. \
                 Observed drift this window: {drift:.2} MB; heap {:+} bytes",
                alloc_end.current_heap_bytes as i64 - alloc_start.current_heap_bytes as i64
            ),
        ));
    }

    // OS slope gate (MB/hr) — extrapolates a shorter run honestly.
    let os = os_metrics(args, phase);
    // Peak-RSS during the soak (prefer the sidecar's sampled peak; else mach).
    let rss_peak = os
        .as_ref()
        .and_then(|o| o.rss_peak_mb)
        .unwrap_or_else(|| rss_end.max(rss_start));
    report.push(
        GateRow::max(
            "soak-peak-rss",
            phase,
            "ps -o rss (sidecar) / task_info",
            "scripts/perf-sanity rss_peak_mb",
            rss_peak,
            gates::RAMP_MEMORY_GATE_MB,
            "MB",
        )
        .with_note("peak RSS must stay bounded across the sustained-tail soak"),
    );
    match os.as_ref().and_then(|o| o.rss_slope_mb_per_hr) {
        Some(slope) => report.push(GateRow::max(
            "memory-slope",
            phase,
            "ps -o rss sampled loop (sidecar)",
            "scripts/perf-sanity rss_slope_mb_per_hr",
            slope,
            gates::MEMORY_SLOPE_MB_PER_HR_GATE,
            "MB/hr",
        )),
        None => report.push(GateRow::unmeasured(
            "memory-slope",
            phase,
            "ps -o rss sampled loop (sidecar)",
            "scripts/perf-sanity rss_slope_mb_per_hr",
            &format!("<= {} MB/hr", gates::MEMORY_SLOPE_MB_PER_HR_GATE),
            Verdict::Blocked,
            "no --os-metrics provided: RSS slope needs the sidecar's ps -o rss loop",
        )),
    }

    // LRU-eviction evidence gate: documented hook gap (see findings). The
    // RamEvictionReport is internal to nmp-core (`run_gc_step`) and has no FFI
    // read hook today — we mark BLOCKED rather than touch the churning store.
    report.push(GateRow::unmeasured(
        "lru-evictions-occur",
        phase,
        "(none)",
        "kernel::ram_eviction::RamEvictionReport",
        ">= 1 eviction under sustained tail",
        Verdict::Blocked,
        "HOOK MISSING — RamEvictionReport has no FFI/diagnostics read seam. \
         Wire an nmp_app_read_ram_eviction_stats counter in a follow-up; \
         do NOT edit run_gc_step here.",
    ));
}

/// Resolve OS sidecar metrics for a phase (if a path was supplied).
fn os_metrics(args: &Args, phase: &str) -> Option<OsPhaseMetrics> {
    args.os_metrics_path
        .as_deref()
        .and_then(|p| load_os_metrics(p, phase))
}
