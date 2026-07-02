//! NMP Gallery TUI — live-only kernel-driven Nostr component showcase.
//!
//! Boots the live kernel, blocks on input/snapshot channels, and redraws from
//! pushed snapshots. Renderers resolve embed URIs through the kernel sink.

use std::{
    cell::RefCell,
    collections::BTreeSet,
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
    data::{GalleryData, LiveProfileMap},
    embed_host::EmbedHostState,
    gallery,
    live::{primary_pubkey, GalleryTypedSnapshot, LiveGallerySource, LiveKernel, LiveKernelSink},
    profile_claim::VisibleProfileClaims,
    render::{self, EmbedFrameContext},
};
use ratatui::{backend::CrosstermBackend, Terminal};

const EMBED_CONSUMER_ID: &str = "nmp-gallery-tui.embed";

mod smoke;
mod smoke_display;

struct Args {
    component: String,
    dump_lines: bool,
    list: bool,
    /// Headless verification mode for renderer-triggered event-ref resolves.
    smoke: bool,
    smoke_timeout_secs: u64,
}

enum GalleryEvent {
    Input(Event),
    Snapshot(Box<GalleryTypedSnapshot>),
    Quit,
}

fn main() -> io::Result<()> {
    let args = parse_args();
    if args.list {
        for component in gallery::component_ids() {
            println!("{component}");
        }
        return Ok(());
    }

    // Smoke mode bypasses the cold-start bootstrap (which can flake when
    // specific hardcoded event ids aren't available on configured relays).
    // It directly validates the embed architecture: kernel boot → renderer
    // resolves via sink -> snapshot delivery -> host decode.
    if args.smoke {
        let mut kernel = match LiveGallerySource::boot_kernel_only() {
            Ok(k) => k,
            Err(error) => {
                eprintln!("failed to boot kernel: {error}");
                std::process::exit(1);
            }
        };
        let sink: Arc<LiveKernelSink> = Arc::new(LiveKernelSink { app: kernel.app });
        let mut host = EmbedHostState::new();
        let snapshot_rx = kernel
            .take_receiver()
            .expect("snapshot receiver must still be present after boot");
        let exit_code = smoke::run(
            &sink,
            &mut host,
            snapshot_rx,
            Duration::from_secs(args.smoke_timeout_secs),
        );
        drop(kernel);
        std::process::exit(exit_code);
    }

    if !gallery::is_component(&args.component) {
        eprintln!(
            "unknown component `{}`; run `nmp-gallery-tui --list`",
            args.component
        );
        std::process::exit(2);
    }

    // Boot the kernel only — no blocking prefetch. Initial frame uses the same
    // real Nostr references as every gallery surface; snapshots update embeds.
    let mut kernel = match LiveKernel::new() {
        Ok(k) => k,
        Err(error) => {
            eprintln!("failed to boot kernel: {error}");
            std::process::exit(1);
        }
    };

    let data = GalleryData::live_initial(primary_pubkey());

    // Build the renderer's registry sink (forwards event/profile claim
    // lifecycles to the persistent kernel).
    let sink: Arc<LiveKernelSink> = Arc::new(LiveKernelSink { app: kernel.app });
    let mut host = EmbedHostState::new();

    // Reactive profile store. Every snapshot tick feeds this; the user-*
    // components resolve `data.primary_pubkey` through it at render time.
    // No app-side field-by-field copying from the snapshot.
    let mut live_profiles = LiveProfileMap::new();

    if args.dump_lines {
        // Non-TTY mode: just render once to stdout. Embeds will be unresolved
        // because no snapshot has flushed yet — the dump path is for
        // structural inspection, not full reactive verification. An empty
        // `LiveProfileMap` is fine: user-* components fall back to npub_short.
        let profiles = LiveProfileMap::new();
        for line in render::plain_lines(&args.component, &data, &profiles, 96) {
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

    run_terminal(
        &args,
        &data,
        &sink,
        &mut host,
        &mut live_profiles,
        snapshot_rx,
    )?;

    // Kernel drops here at end of scope — clears the update callback and
    // frees the app.
    drop(kernel);
    Ok(())
}

fn parse_args() -> Args {
    let mut component = "content-view".to_string();
    let mut dump_lines = false;
    let mut list = false;
    let mut smoke = false;
    let mut smoke_timeout_secs = 30u64;

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
            "--smoke" => smoke = true,
            "--smoke-timeout-secs" => {
                if let Some(value) = iter.next().and_then(|v| v.parse::<u64>().ok()) {
                    smoke_timeout_secs = value;
                }
            }
            value if !value.starts_with('-') => component = value.to_string(),
            _ => {}
        }
    }

    Args {
        component,
        dump_lines,
        list,
        smoke,
        smoke_timeout_secs,
    }
}

fn run_terminal(
    args: &Args,
    data: &GalleryData,
    sink: &Arc<LiveKernelSink>,
    host: &mut EmbedHostState,
    live_profiles: &mut LiveProfileMap,
    snapshot_rx: std::sync::mpsc::Receiver<Vec<u8>>,
) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = drive(
        &mut terminal,
        args,
        data,
        sink,
        host,
        live_profiles,
        snapshot_rx,
    );

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
    live_profiles: &mut LiveProfileMap,
    snapshot_rx: std::sync::mpsc::Receiver<Vec<u8>>,
) -> io::Result<()> {
    let mut selected_index = gallery::component_index(&args.component);
    let mut profile_claims = VisibleProfileClaims::default();

    // Single channel multiplexing input + snapshot. Both threads block on
    // their respective sources (no polling, D8). The main loop blocks on
    // this channel's recv — edge-triggered redraws only.
    let (tx, rx) = mpsc::channel::<GalleryEvent>();
    spawn_input_thread(tx.clone());
    spawn_snapshot_thread(tx.clone(), snapshot_rx);

    draw(
        terminal,
        selected_index,
        data,
        sink,
        host,
        live_profiles,
        &mut profile_claims,
    )?;

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
                draw(
                    terminal,
                    selected_index,
                    data,
                    sink,
                    host,
                    live_profiles,
                    &mut profile_claims,
                )?;
            }
            Ok(GalleryEvent::Input(Event::Resize(_, _))) => {
                draw(
                    terminal,
                    selected_index,
                    data,
                    sink,
                    host,
                    live_profiles,
                    &mut profile_claims,
                )?;
            }
            Ok(GalleryEvent::Input(_)) => {
                // Other input events (mouse, etc.) — ignore.
            }
            Ok(GalleryEvent::Snapshot(snapshot)) => {
                let new_authors = host.update_from_typed(&snapshot);
                live_profiles.update_from_typed(&snapshot);
                resolve_profiles_for(sink, &new_authors);
                // Coalesce any additional snapshots that have already piled
                // up so we don't redraw N times for N quick ticks. Latest
                // wins (the host replaces its state from each tick).
                while let Ok(extra) = rx.try_recv() {
                    match extra {
                        GalleryEvent::Snapshot(next) => {
                            let more = host.update_from_typed(&next);
                            live_profiles.update_from_typed(&next);
                            resolve_profiles_for(sink, &more);
                        }
                        other => {
                            // A non-snapshot event landed during coalescing —
                            // re-queue would deadlock; handle it next loop
                            // by pushing it back via a tiny side-channel.
                            // Simpler: dispatch inline.
                            match other {
                                GalleryEvent::Input(ev) => {
                                    // Recurse-ish: just handle right after redraw.
                                    handle_input_after_snapshot(ev, &mut selected_index);
                                }
                                GalleryEvent::Quit => {
                                    return draw_then_quit(
                                        terminal,
                                        selected_index,
                                        data,
                                        sink,
                                        host,
                                        live_profiles,
                                    )
                                }
                                GalleryEvent::Snapshot(_) => unreachable!(),
                            }
                            break;
                        }
                    }
                }
                draw(
                    terminal,
                    selected_index,
                    data,
                    sink,
                    host,
                    live_profiles,
                    &mut profile_claims,
                )?;
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

/// Fire `resolve_profile` for each event-ref author. `refs.event`
/// carries raw pubkeys only, so the profile components own kind:0 hydration
/// through the normal per-(pubkey, consumer_id) refcounted resolve path.
fn resolve_profiles_for(sink: &Arc<LiveKernelSink>, authors: &[String]) {
    for pubkey in authors {
        sink.resolve_profile(pubkey, "nmp-gallery-tui.embed.author");
    }
}

fn draw_then_quit(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    selected_index: usize,
    data: &GalleryData,
    sink: &Arc<LiveKernelSink>,
    host: &mut EmbedHostState,
    live_profiles: &LiveProfileMap,
) -> io::Result<()> {
    let mut profile_claims = VisibleProfileClaims::default();
    draw(
        terminal,
        selected_index,
        data,
        sink,
        host,
        live_profiles,
        &mut profile_claims,
    )?;
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

fn spawn_snapshot_thread(tx: Sender<GalleryEvent>, rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    thread::spawn(move || {
        // ADR-0070 (#1671): the stateful refs row-delta mirrors live in the
        // snapshot thread (the reader of the kernel frames), merged across
        // ticks so per-key deltas accumulate. They are the sole app-side stores
        // (D4); each frame materialises their current sets into the snapshot.
        let mut ref_profiles = nmp_core::refs::RefProfileStore::new();
        let mut ref_events = nmp_core::refs::RefEventStore::new();
        for frame_bytes in rx {
            let snap = GalleryTypedSnapshot::from_frame_bytes(
                &frame_bytes,
                &mut ref_profiles,
                &mut ref_events,
            );
            if tx.send(GalleryEvent::Snapshot(Box::new(snap))).is_err() {
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
    live_profiles: &LiveProfileMap,
    profile_claims: &mut VisibleProfileClaims,
) -> io::Result<()> {
    let frame_profile_claims = RefCell::new(BTreeSet::new());
    let embed_ctx = EmbedFrameContext {
        envelopes: host.current_envelopes(),
        sink: Some(sink.as_ref()),
        profile_claims: Some(&frame_profile_claims),
        consumer_id: EMBED_CONSUMER_ID,
        profiles: live_profiles,
    };
    terminal.draw(|frame| {
        frame.render_widget(
            gallery::GalleryView::new(selected_index, data, embed_ctx),
            frame.area(),
        )
    })?;
    profile_claims.reconcile(sink, frame_profile_claims.into_inner());
    // Avoid unused-Result lint when channel is dropped during coalesce.
    let _ = TryRecvError::Empty;
    Ok(())
}
