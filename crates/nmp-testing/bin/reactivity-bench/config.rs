pub(crate) const LOOKUP_P99_GATE_NS: u128 = 100_000;
pub(crate) const RECOMPUTE_P99_GATE_NS: u128 = 1_000_000;
pub(crate) const EMIT_HZ_GATE: f64 = 60.0;
pub(crate) const DELTAS_PER_VIEW_SEC_GATE: f64 = 60.0;
pub(crate) const FALSE_WAKEUP_RATE_GATE: f64 = 0.10;
pub(crate) const CANDIDATES_PER_DELTA_GATE: f64 = 1.25;
pub(crate) const WORKING_SET_MEMORY_GATE_BYTES: usize = 100 * 1024 * 1024;
pub(crate) const HOT_EVENT_LIMIT: usize = 10_000;
pub(crate) const WORKING_SET_TARGET_VIEWS: usize = 100;
pub(crate) const CACHED_EVENT_TARGET: usize = 1_000_000;
pub(crate) const FLUSH_INTERVAL_NS: u64 = 16_666_667;
pub(crate) const DELTA_FLUSH_THRESHOLD: usize = 256;
pub(crate) const ALLOCATION_WARMUP_EVENTS: usize = 1_000;

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
}

pub(crate) struct Args {
    pub(crate) scale: Scale,
    pub(crate) write_report: bool,
    pub(crate) fail_on_gate: bool,
}

impl Args {
    pub(crate) fn parse() -> Self {
        let mut scale = Scale::Standard;
        let mut write_report = true;
        let mut fail_on_gate = false;

        let mut iter = std::env::args().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--quick" => scale = Scale::Quick,
                "--standard" => scale = Scale::Standard,
                "--no-write-report" => write_report = false,
                "--fail-on-gate" => fail_on_gate = true,
                "--scale" => {
                    if let Some(value) = iter.next() {
                        scale = match value.as_str() {
                            "quick" => Scale::Quick,
                            "standard" => Scale::Standard,
                            other => {
                                eprintln!("unknown scale `{other}`, expected quick or standard");
                                std::process::exit(64);
                            }
                        };
                    }
                }
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
            scale,
            write_report,
            fail_on_gate,
        }
    }
}

pub(crate) fn print_help() {
    println!(
        "reactivity-bench [--quick|--standard|--scale quick|standard] [--no-write-report] [--fail-on-gate]"
    );
}

#[derive(Clone, Copy)]
pub(crate) struct ScenarioConfig {
    pub(crate) name: &'static str,
    pub(crate) cached_events: usize,
    pub(crate) hot_event_limit: usize,
    pub(crate) replay_events: usize,
    pub(crate) event_rate_per_sec: u64,
    pub(crate) author_count: u32,
    pub(crate) view_mix: ViewMix,
    pub(crate) stream: StreamKind,
}

#[derive(Clone, Copy)]
pub(crate) enum ViewMix {
    QuietIdle,
    FollowingTimeline,
    HashtagFirehose,
    ProfileFanout,
    ThreadBlowup,
    AccountSwitch,
    WorkingSet100Views,
}

#[derive(Clone, Copy)]
pub(crate) enum StreamKind {
    Mixed,
    Hashtag,
    ProfileForSharedAuthor,
    ThreadEvents,
    AccountSwitch,
}

pub(crate) fn scenarios(scale: Scale) -> Vec<ScenarioConfig> {
    let multiplier = match scale {
        Scale::Quick => 1,
        Scale::Standard => 10,
    };

    vec![
        ScenarioConfig {
            name: "quiet_idle",
            cached_events: 10_000,
            hot_event_limit: HOT_EVENT_LIMIT,
            replay_events: 100 * multiplier,
            event_rate_per_sec: 1,
            author_count: 1_000,
            view_mix: ViewMix::QuietIdle,
            stream: StreamKind::Mixed,
        },
        ScenarioConfig {
            name: "following_timeline_scroll",
            cached_events: 100_000,
            hot_event_limit: HOT_EVENT_LIMIT,
            replay_events: 1_000 * multiplier,
            event_rate_per_sec: 100,
            author_count: 2_000,
            view_mix: ViewMix::FollowingTimeline,
            stream: StreamKind::Mixed,
        },
        ScenarioConfig {
            name: "hashtag_firehose",
            cached_events: match scale {
                Scale::Quick => 100_000,
                Scale::Standard => CACHED_EVENT_TARGET,
            },
            hot_event_limit: HOT_EVENT_LIMIT,
            replay_events: 2_000 * multiplier,
            event_rate_per_sec: 2_000,
            author_count: 10_000,
            view_mix: ViewMix::HashtagFirehose,
            stream: StreamKind::Hashtag,
        },
        ScenarioConfig {
            name: "profile_fanout",
            cached_events: 10_000,
            hot_event_limit: HOT_EVENT_LIMIT,
            replay_events: 500 * multiplier,
            event_rate_per_sec: 100,
            author_count: 1_500,
            view_mix: ViewMix::ProfileFanout,
            stream: StreamKind::ProfileForSharedAuthor,
        },
        ScenarioConfig {
            name: "thread_blowup",
            cached_events: 10_000,
            hot_event_limit: HOT_EVENT_LIMIT,
            replay_events: match scale {
                Scale::Quick => 1_100,
                Scale::Standard => 5_500,
            },
            event_rate_per_sec: 500,
            author_count: 2_000,
            view_mix: ViewMix::ThreadBlowup,
            stream: StreamKind::ThreadEvents,
        },
        ScenarioConfig {
            name: "account_switch",
            cached_events: 10_000,
            hot_event_limit: HOT_EVENT_LIMIT,
            replay_events: 10,
            event_rate_per_sec: 10,
            author_count: 2_000,
            view_mix: ViewMix::AccountSwitch,
            stream: StreamKind::AccountSwitch,
        },
        ScenarioConfig {
            name: "working_set_100_views",
            cached_events: match scale {
                Scale::Quick => 100_000,
                Scale::Standard => CACHED_EVENT_TARGET,
            },
            hot_event_limit: HOT_EVENT_LIMIT,
            replay_events: 1_000 * multiplier,
            event_rate_per_sec: 200,
            author_count: 10_000,
            view_mix: ViewMix::WorkingSet100Views,
            stream: StreamKind::Mixed,
        },
    ]
}
