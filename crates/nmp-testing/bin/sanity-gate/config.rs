//! Absolute sanity gates + CLI args for the new-architecture pressure test.
//!
//! GOAL (not a before/after comparison): detect whether the CURRENT
//! architecture MISBEHAVES under real load — CPU pegging / busy-spin / polling,
//! memory leaks (unbounded RSS), latency cliffs, dropped events, or correctness
//! breaks. These are ABSOLUTE thresholds, not deltas.
//!
//! Where a latency gate already exists in the firehose-bench it is REUSED
//! verbatim (cited below) rather than reinvented — see
//! `crates/nmp-testing/bin/firehose-bench/config.rs`.

/// CPU/spin/memory gate thresholds. Latency gates that mirror firehose-bench
/// carry a `// firehose-bench/config.rs:N` citation so the single source of
/// truth stays visible.
pub mod gates {
    // ── idle-CPU gate (the user's headline concern) ────────────────────────
    /// With ZERO events arriving (connected + EOSE'd), sustained process CPU
    /// must stay below this. Catches spin/poll loops. Sampled by the OS
    /// sidecar (`ps`/`top -H`) over the idle-soak window.
    pub const IDLE_CPU_PCT_GATE: f64 = 2.0;

    // ── no-spin gate ───────────────────────────────────────────────────────
    /// Any single thread pegged above this % while the ingest queue is empty
    /// is a busy-loop. Sampled per-thread via `top -H` / `ps -M`.
    pub const NO_SPIN_PER_THREAD_PCT_GATE: f64 = 90.0;

    // ── load-CPU sanity (not a hard fail, but flagged) ─────────────────────
    /// Under sustained firehose, total process CPU above this is "hot" — a
    /// soft flag (a real firehose legitimately uses CPU; a value near a full
    /// core *while idle* is the bug we hunt). Informational gate.
    pub const LOAD_CPU_PCT_SOFT_FLAG: f64 = 150.0;

    // ── memory gates ───────────────────────────────────────────────────────
    /// RSS slope ceiling over a ≥30-min sustained-tail soak. Mirrors the
    /// intent of `MEMORY_DRIFT_30M_GATE_MB = 50` in firehose-bench/config.rs:4
    /// but expressed as a RATE so shorter dev runs can extrapolate.
    pub const MEMORY_SLOPE_MB_PER_HR_GATE: f64 = 100.0; // ≈ firehose 50MB/30m
    /// Absolute drift ceiling for a full 30-min soak (firehose-bench/config.rs:4).
    pub const MEMORY_DRIFT_30M_GATE_MB: f64 = 50.0;
    /// Peak RSS ceiling during the ramp (firehose-bench/config.rs:3).
    pub const RAMP_MEMORY_GATE_MB: f64 = 200.0;

    // ── latency gates (REUSED verbatim from firehose-bench/config.rs) ──────
    /// Cold-start time-to-first-item (firehose-bench/config.rs:1).
    pub const FIRST_ITEM_GATE_MS: f64 = 800.0;
    /// Cold-start time-to-filled-timeline (firehose-bench/config.rs:2).
    pub const FILLED_TIMELINE_GATE_MS: f64 = 5_000.0;
    /// Ingest→emit p99 (firehose-bench/config.rs:5).
    pub const INGEST_TO_EMIT_P99_GATE_MS: f64 = 50.0;
    /// Load-older (paginate back) latency. No firehose constant exists; this
    /// is a NEW absolute gate at the same order as filled-timeline.
    pub const LOAD_OLDER_GATE_MS: f64 = 5_000.0;

    // ── robustness-oracle gates (families 1/4) ─────────────────────────────
    /// Reactive wire-to-visible latency ceiling: from injecting a batch of
    /// matching events through the verify→store→wakeup→project→emit path until
    /// every one is reflected in the view's projection. Absolute, not a delta.
    /// Generous because it covers the FULL reactive pipeline (not a single tick)
    /// and runs on CI hardware; a wakeup-storm/poll regression blows past it.
    pub const WIRE_TO_VISIBLE_GATE_MS: f64 = 10_000.0;
    /// FFI frame-boundedness ceiling (ADR-0044 "the full store never crosses
    /// FFI"): the raw FlatBuffers snapshot frame must stay below this regardless
    /// of how large the underlying store grows. A frame that scales with store
    /// size is the unbounded-projection bug this gate hunts.
    pub const FRAME_BYTES_GATE: f64 = 4_000_000.0;
}

/// How many distinct timeline items a "filled" cold-start requires. Matches the
/// firehose live cold_start threshold (`visible >= 200`).
pub const FILLED_TIMELINE_TARGET: u64 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Cold-start: first-item + filled-timeline latency.
    ColdStart,
    /// Idle soak: connected, EOSE'd, no new events — measure CPU/spin ≥60s.
    IdleSoak,
    /// Memory soak: sustained tailing, RSS slope + LRU-eviction evidence.
    MemorySoak,
    /// Firehose: sustained ingest, ingest→emit p99, dropped-events oracle.
    Firehose,
    /// Correctness oracles under load (follow-set, dedup, supersession).
    Correctness,
    /// Reactive-correctness oracles (missed-update, wire-to-visible latency,
    /// no-duplicate-stored-row) — robustness family 1.
    Reactive,
    /// Relay-resilience / chaos oracles (store-serves-while-relay-dead,
    /// sub-leak, outbox routing) — robustness family 2.
    Resilience,
    /// Privacy / security oracles (gift-wrap never republished, unverified
    /// events rejected, pre-verified bypass test-gated) — robustness family 3.
    Privacy,
    /// FFI-boundedness + panic-safety oracles — robustness family 4.
    FfiBounds,
    /// GC coverage-hole soundness oracle (LRU opt-in) — robustness family 5.
    GcSoundness,
    /// All of the above in sequence.
    All,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::ColdStart => "cold_start",
            Phase::IdleSoak => "idle_soak",
            Phase::MemorySoak => "memory_soak",
            Phase::Firehose => "firehose",
            Phase::Correctness => "correctness",
            Phase::Reactive => "reactive",
            Phase::Resilience => "resilience",
            Phase::Privacy => "privacy",
            Phase::FfiBounds => "ffi_bounds",
            Phase::GcSoundness => "gc_soundness",
            Phase::All => "all",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "cold_start" => Phase::ColdStart,
            "idle_soak" => Phase::IdleSoak,
            "memory_soak" => Phase::MemorySoak,
            "firehose" => Phase::Firehose,
            "correctness" => Phase::Correctness,
            "reactive" => Phase::Reactive,
            "resilience" => Phase::Resilience,
            "privacy" => Phase::Privacy,
            "ffi_bounds" => Phase::FfiBounds,
            "gc_soundness" => Phase::GcSoundness,
            "all" => Phase::All,
            _ => return None,
        })
    }

    /// Expand `All` into the concrete ordered phase list the runner executes.
    pub fn expand(self) -> Vec<Phase> {
        match self {
            Phase::All => vec![
                Phase::ColdStart,
                Phase::IdleSoak,
                Phase::Firehose,
                Phase::Correctness,
                Phase::Reactive,
                Phase::Resilience,
                Phase::Privacy,
                Phase::FfiBounds,
                Phase::GcSoundness,
                Phase::MemorySoak,
            ],
            other => vec![other],
        }
    }
}

pub struct Args {
    /// Which phase(s) to run.
    pub phase: Phase,
    /// Relay URL the driver connects the kernel to. Defaults to the local
    /// `nak serve` port used by the orchestrator.
    pub relay: String,
    /// `--live`: drive real public relays instead of the local nak relay.
    /// (The relay set is still passed via `--relay`; this flag only changes
    /// the report's `mode` field + SKIP semantics.)
    pub live: bool,
    /// nsec (or `nsec1…` / 64-hex) to sign in as. Empty → ephemeral key.
    pub nsec: Option<String>,
    /// Viewer hex pubkey for the follow-set oracle (the account's own key).
    pub viewer_hex: Option<String>,
    /// Expected follow count for the 2k-follow oracle (from the account's
    /// latest kind:3 p-tag count, resolved by the orchestrator). When < the
    /// floor, the correctness phase marks the fixture UNAVAILABLE and SKIPs
    /// LOUD rather than diluting the threshold.
    pub follow_count: Option<u64>,
    /// Soak duration override in seconds (idle/memory). Default keeps dev runs
    /// short; the orchestrator passes the full 1800s for a real soak.
    pub soak_secs: u64,
    /// Output run directory under `docs/perf/<run>/`.
    pub run_id: String,
    /// Where the OS sidecar wrote its CPU/RSS JSON (merged into the report).
    pub os_metrics_path: Option<String>,
    /// Exit non-zero if any gate FAILs (not SKIP).
    pub fail_on_gate: bool,
}

impl Args {
    pub fn parse() -> Self {
        let mut phase = Phase::All;
        let mut relay = "ws://127.0.0.1:10547".to_string();
        let mut live = false;
        let mut nsec = None;
        let mut viewer_hex = None;
        let mut follow_count = None;
        let mut soak_secs = 60;
        let mut run_id = default_run_id();
        let mut os_metrics_path = None;
        let mut fail_on_gate = false;

        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--phase" => {
                    let v = it.next().unwrap_or_default();
                    phase = Phase::parse(&v).unwrap_or_else(|| {
                        eprintln!("unknown phase `{v}`");
                        std::process::exit(64);
                    });
                }
                "--relay" => relay = it.next().unwrap_or(relay),
                "--live" => live = true,
                "--nsec" => nsec = it.next(),
                "--viewer-hex" => viewer_hex = it.next(),
                "--follow-count" => {
                    follow_count = it.next().and_then(|v| v.parse().ok());
                }
                "--soak-secs" => {
                    soak_secs = it.next().and_then(|v| v.parse().ok()).unwrap_or(soak_secs);
                }
                "--run-id" => run_id = it.next().unwrap_or(run_id),
                "--os-metrics" => os_metrics_path = it.next(),
                "--fail-on-gate" => fail_on_gate = true,
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
            phase,
            relay,
            live,
            nsec,
            viewer_hex,
            follow_count,
            soak_secs,
            run_id,
            os_metrics_path,
            fail_on_gate,
        }
    }
}

fn default_run_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("sanity-{stamp}")
}

pub fn print_help() {
    println!(
        "sanity-gate — absolute new-architecture sanity gates\n\
         \n\
         USAGE:\n\
         \x20 sanity-gate [--phase P] [--relay URL] [--live] [--nsec N] \\\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20[--viewer-hex HEX] [--follow-count N] [--soak-secs S] \\\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20[--run-id ID] [--os-metrics PATH] [--fail-on-gate]\n\
         \n\
         PHASES: cold_start | idle_soak | memory_soak | firehose | correctness |\n\
         \x20\x20\x20\x20\x20\x20\x20\x20 reactive | resilience | privacy | ffi_bounds | gc_soundness | all\n\
         \n\
         Drives the real kernel via the public nmp_app_* FFI (sign-in-as-account,\n\
         add relay, chirp home feed). Reads in-process numbers via the existing\n\
         hooks (decode_snapshot_envelope, churn stats, CountingAllocator) and CPU/\n\
         RSS via the OS sidecar JSON merged from --os-metrics.\n\
         Honest-validation: SKIP LOUD on relay miss, never fake green."
    );
}
