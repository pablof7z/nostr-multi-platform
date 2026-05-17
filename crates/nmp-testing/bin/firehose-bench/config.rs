pub(crate) const FIRST_ITEM_GATE_MS: f64 = 800.0;
pub(crate) const FILLED_TIMELINE_GATE_MS: f64 = 5_000.0;
pub(crate) const RAMP_MEMORY_GATE_MB: f64 = 200.0;
pub(crate) const MEMORY_DRIFT_30M_GATE_MB: f64 = 50.0;
pub(crate) const INGEST_TO_EMIT_P99_GATE_MS: f64 = 50.0;
pub(crate) const VIEW_BATCH_HZ_GATE: f64 = 60.0;
pub(crate) const DELTAS_PER_VIEW_SEC_GATE: f64 = 60.0;
pub(crate) const DISCONNECT_DETECT_GATE_MS: f64 = 10_000.0;
pub(crate) const RECONNECT_GATE_MS: f64 = 30_000.0;
pub(crate) const NIP77_BYTES_RATIO_GATE: f64 = 0.05;
pub(crate) const NSE_DECRYPT_GATE_MS: f64 = 200.0;
pub(crate) const NSE_MEMORY_GATE_MB: f64 = 24.0;
pub(crate) const SOAK_MEMORY_GATE_MB: f64 = 100.0;

#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Replay,
    Capture,
    Live,
}

impl Mode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Mode::Replay => "replay",
            Mode::Capture => "capture",
            Mode::Live => "live",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Scale {
    Quick,
    Standard,
}

impl Scale {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Scale::Quick => "quick",
            Scale::Standard => "standard",
        }
    }

    pub(crate) fn factor(self) -> f64 {
        match self {
            Scale::Quick => 0.2,
            Scale::Standard => 1.0,
        }
    }
}

pub(crate) struct Args {
    pub(crate) mode: Mode,
    pub(crate) scale: Scale,
    pub(crate) scenario: Option<String>,
    pub(crate) write_report: bool,
    pub(crate) fail_on_gate: bool,
}

impl Args {
    pub(crate) fn parse() -> Self {
        let mut args = std::env::args().skip(1).collect::<Vec<_>>();
        let mode = match args.first().map(String::as_str) {
            Some("replay") => {
                args.remove(0);
                Mode::Replay
            }
            Some("capture") => {
                args.remove(0);
                Mode::Capture
            }
            Some("live") => {
                args.remove(0);
                Mode::Live
            }
            _ => Mode::Replay,
        };

        let mut scale = Scale::Standard;
        let mut scenario = None;
        let mut write_report = true;
        let mut fail_on_gate = false;
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--quick" => scale = Scale::Quick,
                "--standard" => scale = Scale::Standard,
                "--scenario" => scenario = iter.next(),
                "--no-write-report" => write_report = false,
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
            mode,
            scale,
            scenario,
            write_report,
            fail_on_gate,
        }
    }
}

pub(crate) fn print_help() {
    println!(
        "firehose-bench [replay|capture|live] [--quick|--standard] [--scenario name] [--no-write-report] [--fail-on-gate]"
    );
}

pub(crate) fn selected_scenarios(selected: Option<&str>) -> Vec<&'static str> {
    let all = vec![
        "cold_start",
        "sustained_firehose",
        "profile_thrashing",
        "relay_disconnect_storm",
        "multi_account",
        "negentropy_efficiency",
        "background_decryption",
        "soak",
    ];
    match selected {
        Some("all") | None => all,
        Some(name) if all.contains(&name) => {
            vec![all.into_iter().find(|item| *item == name).unwrap()]
        }
        Some(name) => {
            eprintln!("unknown scenario `{name}`");
            std::process::exit(64);
        }
    }
}

/// M1 live scenarios: only `cold_start` and `profile_thrashing` are
/// implemented in live mode.  Other scenarios require features that belong to
/// later milestones (LMDB, NIP-77, NIP-65, NSE, multi-account).
pub(crate) fn selected_live_scenarios(selected: Option<&str>) -> Vec<&'static str> {
    let all = vec!["cold_start", "profile_thrashing"];
    match selected {
        Some("all") | None => all,
        Some(name) if all.contains(&name) => {
            vec![all.into_iter().find(|item| *item == name).unwrap()]
        }
        Some(name) => {
            eprintln!("unknown live scenario `{name}` (M1 supports: cold_start, profile_thrashing)");
            std::process::exit(64);
        }
    }
}
