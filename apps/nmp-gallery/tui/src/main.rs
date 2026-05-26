//! NMP Gallery TUI — live-only kernel-driven Nostr component showcase.
//!
//! The program flow:
//! 1. Spin up `LiveKernel` (the persistent `nmp_app_*` actor handle).
//! 2. Cold-start: fetch demo profile + thread/author/media items via
//!    `LiveGallerySource::bootstrap` so user-* component pages render
//!    real kind:0 / kind:1 data on the first frame.
//! 3. Take the snapshot receiver off the kernel; spawn two threads:
//!    - input thread (crossterm `event::read` blocking)
//!    - snapshot thread (snapshot push receiver blocking)
//!    Both feed a single `Receiver<GalleryEvent>` the main loop blocks on.
//! 4. Main loop:
//!    - On `Input` → mutate selection state, redraw.
//!    - On `Snapshot` → update `EmbedHostState`, redraw.
//!    The renderer (NostrContentView) calls `sink.claim(uri, …)` when it
//!    encounters embedded URIs; the kernel fetches them (cache or relay);
//!    the next snapshot push delivers them; the redraw shows them.

use std::{
    io,
    sync::{
        mpsc::{self, RecvError, Sender, TryRecvError},
        Arc,
    },
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use nmp_gallery_tui::{
    data::GalleryData,
    embed_host::EmbedHostState,
    gallery,
    live::{parse_snapshot, LiveGallerySource, LiveKernelSink},
    render::{self, EmbedFrameContext},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use serde_json::Value;

const COLD_START_TIMEOUT: Duration = Duration::from_secs(45);
const EMBED_CONSUMER_ID: &str = "nmp-gallery-tui.embed";

struct Args {
    component: String,
    dump_lines: bool,
    list: bool,
}

enum GalleryEvent {
    Input(Event),
    Snapshot(Box<Value>),
    Quit,
}

fn main() -> io::Result<()> {
    let args = parse_args();
    if args.list {
        for component in gallery::COMPONENTS {
            println!("{component}");
        }
        return Ok(());
    }
    if !gallery::is_component(&args.component) {
        eprintln!(
            "unknown component `{}`; run `nmp-gallery-tui --list`",
            args.component
        );
        std::process::exit(2);
    }

    // Cold-start the kernel + bootstrap initial data.
    let source = LiveGallerySource::new(COLD_START_TIMEOUT);
    let (facts, mut kernel) = match source.bootstrap() {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("failed to bootstrap NmpGallery kernel: {error}");
            std::process::exit(1);
        }
    };

    let data = match GalleryData::from_live(&facts, !args.dump_lines) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("failed to build initial NmpGallery data: {error}");
            std::process::exit(1);
        }
    };

    // Build the renderer's embed sink (forwards claim/release to the
    // persistent kernel via the new claim_event FFI). `Arc` so the sink
    // can be passed `&dyn EventClaimSink` to NostrContentView each frame.
    let sink: Arc<LiveKernelSink> = Arc::new(LiveKernelSink { app: kernel.app });
    let mut host = EmbedHostState::new();

    if args.dump_lines {
        // Non-TTY mode: just render once to stdout. Embeds will be unresolved
        // because no snapshot has flushed yet — the dump path is for
        // structural inspection, not full reactive verification.
        for line in render::plain_lines(&args.component, &data, 96) {
            println!("{line}");
        }
        // Drop kernel cleanly.
        drop(kernel);
        return Ok(());
    }

    // Take the snapshot stream off the kernel so the snapshot thread can
    // own it. The kernel's internal `wait_for_*` paths are no longer used
    // after this point — the main loop is the sole consumer.
    let snapshot_rx = kernel
        .take_receiver()
        .expect("snapshot receiver must still be present after bootstrap");

    run_terminal(&args, &data, &sink, &mut host, snapshot_rx)?;

    // Kernel drops here at end of scope — clears the update callback and
    // frees the app.
    drop(kernel);
    Ok(())
}

fn parse_args() -> Args {
    let mut component = "content-view".to_string();
    let mut dump_lines = false;
    let mut list = false;

    let mut iter = std::env::args().skip(1).peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--component" => {
                if let Some(value) = iter.next() {
                    component = value;
                }
            }
            "--dump-lines" => dump_lines = true,
            "--list" => list = true,
            value if !value.starts_with('-') => component = value.to_string(),
            _ => {}
        }
    }

    Args {
        component,
        dump_lines,
        list,
    }
}

fn run_terminal(
    args: &Args,
    data: &GalleryData,
    sink: &Arc<LiveKernelSink>,
    host: &mut EmbedHostState,
    snapshot_rx: std::sync::mpsc::Receiver<String>,
) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = drive(&mut terminal, args, data, sink, host, snapshot_rx);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn drive(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    args: &Args,
    data: &GalleryData,
    sink: &Arc<LiveKernelSink>,
    host: &mut EmbedHostState,
    snapshot_rx: std::sync::mpsc::Receiver<String>,
) -> io::Result<()> {
    let mut selected_index = gallery::component_index(&args.component);

    // Single channel multiplexing input + snapshot. Both threads block on
    // their respective sources (no polling, D8). The main loop blocks on
    // this channel's recv — edge-triggered redraws only.
    let (tx, rx) = mpsc::channel::<GalleryEvent>();
    spawn_input_thread(tx.clone());
    spawn_snapshot_thread(tx.clone(), snapshot_rx);

    draw(terminal, selected_index, data, sink, host)?;

    loop {
        match rx.recv() {
            Ok(GalleryEvent::Quit) => return Ok(()),
            Ok(GalleryEvent::Input(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Down | KeyCode::Char('j') => {
                        let count = gallery::component_count().max(1);
                        selected_index = (selected_index + 1) % count;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        let count = gallery::component_count().max(1);
                        selected_index = (selected_index + count - 1) % count;
                    }
                    KeyCode::Home => selected_index = 0,
                    KeyCode::End => {
                        selected_index = gallery::component_count().saturating_sub(1);
                    }
                    _ => continue, // unknown key — no redraw
                }
                draw(terminal, selected_index, data, sink, host)?;
            }
            Ok(GalleryEvent::Input(Event::Resize(_, _))) => {
                draw(terminal, selected_index, data, sink, host)?;
            }
            Ok(GalleryEvent::Input(_)) => {
                // Other input events (mouse, etc.) — ignore.
            }
            Ok(GalleryEvent::Snapshot(snapshot)) => {
                host.update_from_snapshot(&snapshot);
                // Coalesce any additional snapshots that have already piled
                // up so we don't redraw N times for N quick ticks. Latest
                // wins (the host replaces its state from each tick).
                while let Ok(extra) = rx.try_recv() {
                    match extra {
                        GalleryEvent::Snapshot(next) => host.update_from_snapshot(&next),
                        other => {
                            // A non-snapshot event landed during coalescing —
                            // re-queue would deadlock; handle it next loop
                            // by pushing it back via a tiny side-channel.
                            // Simpler: dispatch inline.
                            match other {
                                GalleryEvent::Input(ev) => {
                                    // Recurse-ish: just handle right after redraw.
                                    handle_input_after_snapshot(
                                        ev,
                                        &mut selected_index,
                                    );
                                }
                                GalleryEvent::Quit => return draw_then_quit(
                                    terminal,
                                    selected_index,
                                    data,
                                    sink,
                                    host,
                                ),
                                GalleryEvent::Snapshot(_) => unreachable!(),
                            }
                            break;
                        }
                    }
                }
                draw(terminal, selected_index, data, sink, host)?;
            }
            Err(RecvError) => return Ok(()),
        }
    }
}

/// During snapshot coalescing we may pull an input event out of order.
/// Process it inline so we don't lose key presses. (Resize doesn't strictly
/// need handling here — the next draw covers it.)
fn handle_input_after_snapshot(ev: Event, selected_index: &mut usize) {
    if let Event::Key(key) = ev {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                let count = gallery::component_count().max(1);
                *selected_index = (*selected_index + 1) % count;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let count = gallery::component_count().max(1);
                *selected_index = (*selected_index + count - 1) % count;
            }
            KeyCode::Home => *selected_index = 0,
            KeyCode::End => {
                *selected_index = gallery::component_count().saturating_sub(1);
            }
            _ => {}
        }
    }
}

fn draw_then_quit(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    selected_index: usize,
    data: &GalleryData,
    sink: &Arc<LiveKernelSink>,
    host: &mut EmbedHostState,
) -> io::Result<()> {
    draw(terminal, selected_index, data, sink, host)?;
    Ok(())
}

fn spawn_input_thread(tx: Sender<GalleryEvent>) {
    thread::spawn(move || loop {
        match event::read() {
            Ok(ev) => {
                if tx.send(GalleryEvent::Input(ev)).is_err() {
                    break;
                }
            }
            Err(_) => {
                let _ = tx.send(GalleryEvent::Quit);
                break;
            }
        }
    });
}

fn spawn_snapshot_thread(
    tx: Sender<GalleryEvent>,
    rx: std::sync::mpsc::Receiver<String>,
) {
    thread::spawn(move || {
        for payload in rx {
            let Some(value) = parse_snapshot(&payload) else {
                continue;
            };
            if tx.send(GalleryEvent::Snapshot(Box::new(value))).is_err() {
                break;
            }
        }
    });
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    selected_index: usize,
    data: &GalleryData,
    sink: &Arc<LiveKernelSink>,
    host: &mut EmbedHostState,
) -> io::Result<()> {
    let sink_ref: &dyn nmp_content::EventClaimSink = sink.as_ref();
    let embed_ctx = EmbedFrameContext {
        envelopes: host.current_envelopes(),
        sink: Some(sink_ref),
        consumer_id: EMBED_CONSUMER_ID,
    };
    terminal.draw(|frame| {
        frame.render_widget(
            gallery::GalleryView::new(selected_index, data, embed_ctx),
            frame.area(),
        )
    })?;
    // Avoid unused-Result lint when channel is dropped during coalesce.
    let _ = TryRecvError::Empty;
    Ok(())
}
