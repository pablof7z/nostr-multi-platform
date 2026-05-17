mod config;
mod report;
mod scenarios;

use config::{selected_scenarios, Args, Mode};
use report::{
    now_unix_seconds, summarize_observations, write_report, write_trace_manifest, FirehoseReport,
};
use scenarios::run_scenario;

fn main() {
    let args = Args::parse();
    let started_at = now_unix_seconds();
    let mut report = FirehoseReport {
        tool: "firehose-bench",
        status: "prototype",
        mode: args.mode.as_str(),
        scale: args.scale.as_str(),
        started_at_unix: started_at,
        scenarios: Vec::new(),
        overall_passed: true,
        limitations: vec![
            "This is a deterministic prototype harness. Real relay sockets, LMDB/SQLite writes, UniFFI marshaling, platform wrappers, and NSE calls are modeled because those runtime pieces do not exist yet.".to_string(),
            "Replay mode is CI-shaped and deterministic. Capture mode currently writes a synthetic trace manifest, not live WebSocket frames.".to_string(),
            "Live mode is intentionally not implemented until the actor, relay adapter, and storage backend exist.".to_string(),
        ],
        observations: Vec::new(),
    };

    if matches!(args.mode, Mode::Live) {
        report.overall_passed = false;
        report.observations.push(
            "Live mode is blocked on real relay adapter, storage backend, and actor integration."
                .to_string(),
        );
    } else {
        let scenario_names = selected_scenarios(args.scenario.as_deref());
        for name in scenario_names {
            let scenario = run_scenario(name, args.scale);
            report.overall_passed &= scenario.passed;
            report.scenarios.push(scenario);
        }

        if matches!(args.mode, Mode::Capture) {
            if let Err(error) = write_trace_manifest(&report) {
                eprintln!("failed to write synthetic trace manifest: {error}");
                std::process::exit(1);
            }
        }
    }

    report.observations.extend(summarize_observations(&report));

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
