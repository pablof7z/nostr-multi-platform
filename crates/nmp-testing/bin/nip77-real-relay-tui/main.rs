mod cache;
mod relay;
mod ui;

use std::env;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::relay::{clear_cache, parsed_cache_count, publish_demo_event, run_sync, Config};
use crate::ui::AppState;

const DEFAULT_GROUP: &str = "nostr-multi-platform";
const DEFAULT_DEMO_TAG: &str = "nmpnip77demo";

enum Input {
    RunBoth,
    RunNeg,
    Publish,
    Clear,
    Redraw,
    Quit,
}

enum Worker {
    Started(String),
    Finished(Result<relay::RunReport, String>),
    Published(Result<relay::PublishReport, String>),
    Cleared(Result<(), String>),
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), String> {
    let args = Args::parse(env::args().skip(1))?;
    let config = Config::new(
        args.relay,
        args.filter_json,
        args.cache_path,
        args.group,
        args.publish_secret,
    );
    if args.once {
        if args.publish_first {
            match publish_demo_event(&config) {
                Ok(publish) => println!(
                    "published {} signer={} accepted={} message={}",
                    relay::id_prefix(&publish.id),
                    relay::id_prefix(&publish.pubkey),
                    publish.accepted,
                    publish.relay_message
                ),
                Err(e) => println!("publish error: {e}"),
            }
        }
        let report = run_sync(&config, !args.neg_only)?;
        print_report(&report);
        return Ok(());
    }
    run_tui(config)
}

fn run_tui(config: Config) -> Result<(), String> {
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
    let result = tui_loop(&mut terminal, config);
    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;
    result
}

fn tui_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    config: Config,
) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<EventOrWorker>();
    spawn_input_thread(tx.clone());
    let mut state = AppState {
        status: format!("idle, cache has {} events", parsed_cache_count(&config)),
        ..AppState::default()
    };
    start_run(config.clone(), tx.clone(), true);
    loop {
        terminal
            .draw(|frame| ui::render(frame, &state, &config))
            .map_err(|e| e.to_string())?;
        match rx.recv().map_err(|e| e.to_string())? {
            EventOrWorker::Input(Input::Quit) => break,
            EventOrWorker::Input(Input::Redraw) => {}
            EventOrWorker::Input(input) if state.running => {
                if matches!(input, Input::Quit) {
                    break;
                }
                state.log("still running; wait for this relay command to finish");
            }
            EventOrWorker::Input(Input::RunBoth) => start_run(config.clone(), tx.clone(), true),
            EventOrWorker::Input(Input::RunNeg) => start_run(config.clone(), tx.clone(), false),
            EventOrWorker::Input(Input::Publish) => start_publish(config.clone(), tx.clone()),
            EventOrWorker::Input(Input::Clear) => start_clear(config.clone(), tx.clone()),
            EventOrWorker::Worker(worker) => apply_worker(worker, &mut state),
        }
    }
    Ok(())
}

enum EventOrWorker {
    Input(Input),
    Worker(Worker),
}

fn spawn_input_thread(tx: mpsc::Sender<EventOrWorker>) {
    thread::spawn(move || loop {
        let input = match event::read() {
            Ok(Event::Key(key)) => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => Input::Quit,
                KeyCode::Char('r') => Input::RunBoth,
                KeyCode::Char('n') => Input::RunNeg,
                KeyCode::Char('p') => Input::Publish,
                KeyCode::Char('c') => Input::Clear,
                _ => continue,
            },
            Ok(Event::Resize(_, _)) => Input::Redraw,
            Ok(_) => continue,
            Err(_) => return,
        };
        if tx.send(EventOrWorker::Input(input)).is_err() {
            return;
        }
    });
}

fn start_run(config: Config, tx: mpsc::Sender<EventOrWorker>, include_plain: bool) {
    let label = if include_plain {
        "running plain REQ + NIP-77"
    } else {
        "running NIP-77 only"
    };
    let _ = tx.send(EventOrWorker::Worker(Worker::Started(label.to_string())));
    thread::spawn(move || {
        let _ = tx.send(EventOrWorker::Worker(Worker::Finished(run_sync(
            &config,
            include_plain,
        ))));
    });
}

fn start_publish(config: Config, tx: mpsc::Sender<EventOrWorker>) {
    let _ = tx.send(EventOrWorker::Worker(Worker::Started(
        "publishing demo event".to_string(),
    )));
    thread::spawn(move || {
        let _ = tx.send(EventOrWorker::Worker(Worker::Published(
            publish_demo_event(&config),
        )));
    });
}

fn start_clear(config: Config, tx: mpsc::Sender<EventOrWorker>) {
    let _ = tx.send(EventOrWorker::Worker(Worker::Started(
        "clearing cache".to_string(),
    )));
    thread::spawn(move || {
        let _ = tx.send(EventOrWorker::Worker(Worker::Cleared(clear_cache(&config))));
    });
}

fn apply_worker(worker: Worker, state: &mut AppState) {
    match worker {
        Worker::Started(label) => {
            state.running = true;
            state.status = label.clone();
            state.log(label);
        }
        Worker::Finished(Ok(report)) => state.apply_run(report),
        Worker::Finished(Err(e)) => {
            state.running = false;
            state.status = "error".to_string();
            state.log(format!("error: {e}"));
        }
        Worker::Published(Ok(report)) => {
            state.running = false;
            state.status = "idle".to_string();
            state.log(format!(
                "published {} as {}",
                relay::id_prefix(&report.id),
                relay::id_prefix(&report.pubkey)
            ));
            state.publish = Some(report);
        }
        Worker::Published(Err(e)) | Worker::Cleared(Err(e)) => {
            state.running = false;
            state.status = "error".to_string();
            state.log(format!("error: {e}"));
        }
        Worker::Cleared(Ok(())) => {
            state.running = false;
            state.status = "cache cleared".to_string();
            state.log("cache cleared");
            state.newest.clear();
        }
    }
}

fn print_report(report: &relay::RunReport) {
    println!("cache: {}", report.cache_path.display());
    println!("surface: {}", report.surface);
    if let Some(plain) = &report.plain {
        println!(
            "plain REQ: events={} bytes_sent={} bytes_received={} elapsed_ms={}",
            plain.events, plain.bytes_sent, plain.bytes_received, plain.elapsed_ms
        );
    }
    if let Some(neg) = &report.neg {
        println!(
            "NIP-77: cache {} -> {}, need={} fetched={} have={} rounds={} bytes_sent={} bytes_received={} elapsed_ms={}",
            neg.local_before,
            neg.local_after,
            neg.need,
            neg.fetched,
            neg.have,
            neg.rounds,
            neg.bytes_sent,
            neg.bytes_received,
            neg.elapsed_ms
        );
    }
    if let Some(error) = &report.neg_error {
        println!("NIP-77 error: {error}");
    }
    for event in &report.newest {
        println!(
            "cached {} kind={} at={} {}",
            relay::id_prefix(&event.id),
            event.kind,
            event.created_at,
            event.content.replace('\n', " ")
        );
    }
}

struct Args {
    relay: String,
    filter_json: String,
    cache_path: Option<PathBuf>,
    group: Option<String>,
    publish_secret: Option<String>,
    once: bool,
    neg_only: bool,
    publish_first: bool,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut filter_was_set = false;
        let mut out = Self {
            relay: default_relay(),
            filter_json: group_filter_json(DEFAULT_GROUP),
            cache_path: None,
            group: Some(DEFAULT_GROUP.to_string()),
            publish_secret: None,
            once: false,
            neg_only: false,
            publish_first: false,
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--relay" => out.relay = take_value(&mut args, "--relay")?,
                "--filter" => {
                    out.filter_json = take_value(&mut args, "--filter")?;
                    filter_was_set = true;
                }
                "--cache" => {
                    out.cache_path = Some(PathBuf::from(take_value(&mut args, "--cache")?))
                }
                "--nsec" => out.publish_secret = Some(take_value(&mut args, "--nsec")?),
                "--group" => {
                    let group = take_value(&mut args, "--group")?;
                    if !filter_was_set {
                        out.filter_json = group_filter_json(&group);
                    }
                    out.group = Some(group);
                }
                "--no-group" => {
                    if !filter_was_set {
                        out.filter_json = tag_filter_json(DEFAULT_DEMO_TAG);
                    }
                    out.group = None;
                }
                "--once" => out.once = true,
                "--neg-only" => out.neg_only = true,
                "--publish-first" => out.publish_first = true,
                "--help" | "-h" => return Err(help()),
                other => return Err(format!("unknown argument: {other}\n\n{}", help())),
            }
        }
        if out.publish_secret.is_none() {
            out.publish_secret = env::var("NMP_NIP77_DEMO_NSEC").ok();
        }
        Ok(out)
    }
}

fn take_value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{name} requires a value\n\n{}", help()))
}

fn help() -> String {
    let default_relay = default_relay();
    format!(
        "usage: cargo run -p nmp-testing --bin nip77-real-relay-tui -- [--relay URL] [--group ID|--no-group] [--filter JSON] [--cache PATH] [--nsec SECRET] [--once] [--neg-only] [--publish-first]\n\
         default relay: {default_relay}\n\
         default group: {DEFAULT_GROUP}\n\
         default filter: {}\n\
         publish signer: ephemeral by default, or NMP_NIP77_DEMO_NSEC / --nsec for a group member key\n\
         TUI keys: r run plain+NIP-77, n NIP-77 only, p publish demo event, c clear cache, q quit",
        group_filter_json(DEFAULT_GROUP)
    )
}

fn group_filter_json(group: &str) -> String {
    serde_json::json!({ "#h": [group] }).to_string()
}

fn tag_filter_json(tag: &str) -> String {
    serde_json::json!({ "#t": [tag] }).to_string()
}

fn default_relay() -> String {
    ["wss://", "nip29.f7z.io"].concat()
}
