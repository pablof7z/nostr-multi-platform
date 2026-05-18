//! ffi-stress — FFI hardening stress harness (M10.5 phase 1).
//!
//! Drives the `nmp_app_*` C symbols via extern declarations (same ABI Swift
//! uses) and verifies S1–S5 gate conditions from
//! `docs/design/ffi-hardening/gates.md`.
//!
//! Usage:
//!   ffi-stress <scenario> [--duration <D>] [--threads <N>] [--fail-on-gate]
//!                         [--write-report]
//!
//! Scenarios: mount-unmount (S1) | dispatch-flood (S2) | snapshot-pressure (S3)
//!            | reconciler-backpressure (S4) | reentrancy (S5)
//!
//! See `docs/design/ffi-hardening/harness.md` §1.2 for the full CLI reference.

mod allocator;
mod common;
mod ffi;
mod gate;
mod report;
mod s1_mount_unmount;
mod s2_dispatch_flood;
mod s3_snapshot_pressure;
mod s4_reconciler_backpressure;
mod s5_reentrancy;

use report::{now_unix_seconds, write_scenario_report, ScenarioMetrics};
use std::process;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cli = match Cli::parse(&args[1..]) {
        Ok(cli) => cli,
        Err(msg) => {
            eprintln!("ffi-stress: {msg}");
            eprintln!("{USAGE}");
            process::exit(1);
        }
    };

    let mut overall_pass = true;

    for scenario in &cli.scenarios {
        let mut metrics = ScenarioMetrics::new(scenario_name(scenario));
        metrics.started_at_unix = now_unix_seconds();

        eprintln!("ffi-stress: running {} …", scenario_name(scenario));

        match scenario {
            Scenario::MountUnmount => {
                let cfg = s1_mount_unmount::S1Config {
                    duration: cli.duration,
                    ..Default::default()
                };
                s1_mount_unmount::run(cfg, &mut metrics);
            }
            Scenario::DispatchFlood => {
                let cfg = s2_dispatch_flood::S2Config {
                    duration: cli.duration,
                    threads: cli.threads,
                    ..Default::default()
                };
                s2_dispatch_flood::run(cfg, &mut metrics);
            }
            Scenario::SnapshotPressure => {
                let cfg = s3_snapshot_pressure::S3Config::default();
                s3_snapshot_pressure::run(cfg, &mut metrics);
            }
            Scenario::ReconcilerBackpressure => {
                let cfg = s4_reconciler_backpressure::S4Config {
                    duration: cli.duration,
                    ..Default::default()
                };
                s4_reconciler_backpressure::run(cfg, &mut metrics);
            }
            Scenario::Reentrancy => {
                let cfg = s5_reentrancy::S5Config {
                    duration: cli.duration,
                    ..Default::default()
                };
                s5_reentrancy::run(cfg, &mut metrics);
            }
        }

        let passed = metrics.passed;
        overall_pass &= passed;

        eprintln!(
            "ffi-stress: {} — {} ({}/{} gates)",
            scenario_name(scenario),
            if passed { "PASS" } else { "FAIL" },
            metrics.gates.iter().filter(|g| g.passed).count(),
            metrics.gates.len()
        );
        for gate in &metrics.gates {
            let sym = if gate.passed { "  ok" } else { "FAIL" };
            eprintln!("  [{sym}] {}: {:.4} (op={:?} threshold={:.4})",
                gate.name, gate.measured, gate.op, gate.threshold);
        }

        if cli.write_report {
            if let Err(e) = write_scenario_report(&metrics) {
                eprintln!("ffi-stress: failed to write report: {e}");
            }
        }

        // Print JSON to stdout for pipeline consumption.
        println!(
            "{}",
            serde_json::to_string_pretty(&metrics).expect("serialize")
        );
    }

    if cli.fail_on_gate && !overall_pass {
        process::exit(2);
    }
}

// ---- CLI parsing -----------------------------------------------------------

const USAGE: &str = r#"Usage: ffi-stress <scenario> [options]

Scenarios:
  mount-unmount            S1 — view-handle wrapper refcount churn
  dispatch-flood           S2 — mpsc backpressure (10k/s, 4 threads)
  snapshot-pressure        S3 — AppUpdate::FullState serialization pressure
  reconciler-backpressure  S4 — 250 ms main-thread stall simulation
  reentrancy               S5 — dispatch from inside reconciler callback

Options:
  --duration <D>           Wall-clock duration (e.g. 60s, 10m). Default: scenario-specific.
  --threads <N>            Caller thread count (S2 default: 4).
  --fail-on-gate           Exit 2 if any gate fails.
  --write-report           Write docs/perf/m10.5/<scenario>/{metrics.json,report.md}.
"#;

#[derive(Debug, Clone, Copy)]
enum Scenario {
    MountUnmount,
    DispatchFlood,
    SnapshotPressure,
    ReconcilerBackpressure,
    Reentrancy,
}

fn scenario_name(s: &Scenario) -> &'static str {
    match s {
        Scenario::MountUnmount => "S1-mount-unmount",
        Scenario::DispatchFlood => "S2-dispatch-flood",
        Scenario::SnapshotPressure => "S3-snapshot-pressure",
        Scenario::ReconcilerBackpressure => "S4-reconciler-backpressure",
        Scenario::Reentrancy => "S5-reentrancy",
    }
}

struct Cli {
    scenarios: Vec<Scenario>,
    duration: Duration,
    threads: usize,
    fail_on_gate: bool,
    write_report: bool,
}

impl Cli {
    fn parse(args: &[String]) -> Result<Cli, String> {
        let mut scenarios: Vec<Scenario> = Vec::new();
        let mut duration: Option<Duration> = None;
        let mut threads: usize = 4;
        let mut fail_on_gate = false;
        let mut write_report = false;

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--duration" => {
                    i += 1;
                    let s = args.get(i).ok_or("--duration requires a value")?;
                    duration = Some(parse_duration(s)?);
                }
                "--threads" => {
                    i += 1;
                    let s = args.get(i).ok_or("--threads requires a value")?;
                    threads = s.parse::<usize>().map_err(|_| "invalid --threads value")?;
                }
                "--fail-on-gate" => fail_on_gate = true,
                "--write-report" => write_report = true,
                s if !s.starts_with('-') => {
                    let sc = parse_scenario(s)?;
                    scenarios.push(sc);
                }
                unknown => return Err(format!("unknown argument: {unknown}")),
            }
            i += 1;
        }

        if scenarios.is_empty() {
            return Err("no scenario specified".to_string());
        }

        // Default durations per scenario (fast mode).
        let duration = duration.unwrap_or_else(|| match scenarios.first() {
            Some(Scenario::MountUnmount) => Duration::from_secs(60),
            Some(Scenario::DispatchFlood) => Duration::from_secs(30),
            Some(Scenario::SnapshotPressure) => Duration::from_secs(30),
            Some(Scenario::ReconcilerBackpressure) => Duration::from_secs(60),
            Some(Scenario::Reentrancy) => Duration::from_secs(30),
            None => Duration::from_secs(30),
        });

        Ok(Cli {
            scenarios,
            duration,
            threads,
            fail_on_gate,
            write_report,
        })
    }
}

fn parse_scenario(s: &str) -> Result<Scenario, String> {
    match s {
        "mount-unmount" | "s1" => Ok(Scenario::MountUnmount),
        "dispatch-flood" | "s2" => Ok(Scenario::DispatchFlood),
        "snapshot-pressure" | "s3" => Ok(Scenario::SnapshotPressure),
        "reconciler-backpressure" | "s4" => Ok(Scenario::ReconcilerBackpressure),
        "reentrancy" | "s5" => Ok(Scenario::Reentrancy),
        other => Err(format!("unknown scenario: {other}")),
    }
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    if let Some(mins) = s.strip_suffix('m') {
        let m: u64 = mins
            .parse()
            .map_err(|_| format!("invalid duration: {s}"))?;
        return Ok(Duration::from_secs(m * 60));
    }
    if let Some(secs) = s.strip_suffix('s') {
        let s: u64 = secs
            .parse()
            .map_err(|_| format!("invalid duration: {s}"))?;
        return Ok(Duration::from_secs(s));
    }
    // Plain number = seconds.
    s.parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|_| format!("invalid duration: {s}"))
}
