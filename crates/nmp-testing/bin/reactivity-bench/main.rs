mod allocator;
mod config;
mod domain;
mod report;
mod rng;
mod scenario;
mod world;

use config::{scenarios, Args};
use report::{now_unix_seconds, write_report, BenchReport};
use scenario::run_scenario;

fn main() {
    let args = Args::parse();
    let started_at = now_unix_seconds();
    let mut report = BenchReport {
        tool: "reactivity-bench",
        started_at_unix: started_at,
        scale: args.scale.as_str(),
        scenarios: Vec::new(),
        overall_passed: true,
        caveats: vec![
            "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.".to_string(),
            "Allocation measurement uses a process-wide counting GlobalAlloc and samples only the insert -> lookup -> recompute -> delta-buffer path after warmup.".to_string(),
        ],
    };

    let scenario_configs = scenarios(args.scale);
    for config in scenario_configs {
        let result = run_scenario(config);
        report.overall_passed &= result.passed;
        report.scenarios.push(result);
    }

    if args.write_report {
        if let Err(error) = write_report(&report) {
            eprintln!("failed to write report: {error}");
            std::process::exit(1);
        }
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serializes report")
    );

    if args.fail_on_gate && !report.overall_passed {
        std::process::exit(2);
    }
}
