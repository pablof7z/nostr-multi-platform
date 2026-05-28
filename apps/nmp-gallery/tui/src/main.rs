//! NMP Gallery TUI — live-only kernel-driven Nostr component showcase.
//!
//! The program flow:
//! 1. Spin up `LiveKernel` (the persistent `nmp_app_*` actor handle).
//! 2. Boot `LiveKernel` without blocking prefetch. The initial frame carries
//!    canonical Nostr references; relay-backed projections refine it in place.
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
use nmp_content::EventClaimSink;
use nmp_gallery_tui::{
    data::{GalleryData, LiveProfileMap},
    embed_host::EmbedHostState,
    gallery,
    live::{parse_snapshot, primary_pubkey, LiveGallerySource, LiveKernel, LiveKernelSink},
    render::{self, EmbedFrameContext},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use serde_json::Value;

const EMBED_CONSUMER_ID: &str = "nmp-gallery-tui.embed";

struct Args {
    component: String,
    dump_lines: bool,
    list: bool,
    /// Headless verification mode — boots the kernel, claims every embed
    /// URI the gallery's content trees reference, waits up to N seconds
    /// for each claim to resolve via the snapshot push, and prints a
    /// structured pass/fail report. Exits 0 on full success, 1 on any
    /// timeout or decode failure. Used to validate the architecture
    /// end-to-end without an interactive terminal.
    smoke: bool,
    smoke_timeout_secs: u64,
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

    // Smoke mode bypasses the cold-start bootstrap (which can flake when
    // specific hardcoded event ids aren't available on configured relays).
    // It directly validates the embed architecture: kernel boot → renderer
    // claims via sink → snapshot delivery → host decode.
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
        let exit_code = run_smoke(
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

/// Headless verification of the renderer-triggered claim path. Mirrors what
/// the TUI does at render time but without ratatui — claims each embed URI
/// directly via the sink, drains snapshots into the host until either the
/// targets resolve or the timeout fires, then prints a structured report.
fn run_smoke(
    sink: &Arc<LiveKernelSink>,
    host: &mut EmbedHostState,
    snapshot_rx: std::sync::mpsc::Receiver<String>,
    timeout: Duration,
) -> i32 {
    use nmp_core::nip19::{decode_naddr, decode_nevent, decode_note};
    use std::time::Instant;

    struct SmokeTarget {
        label: &'static str,
        uri: String,
        /// Snapshot key the kernel uses for `claimed_events[primary_id]`.
        /// hex64 event id for nevent/note; "kind:author:d_tag" for naddr.
        primary_id: String,
    }

    fn primary_id_for(uri: &str) -> Option<String> {
        let stripped = uri.strip_prefix("nostr:").unwrap_or(uri);
        if let Ok(naddr) = decode_naddr(stripped) {
            return Some(format!(
                "{}:{}:{}",
                naddr.kind, naddr.pubkey, naddr.identifier
            ));
        }
        if let Ok(nevent) = decode_nevent(stripped) {
            return Some(nevent.event_id);
        }
        if let Ok(note) = decode_note(stripped) {
            return Some(note);
        }
        None
    }

    // The smoke uses the same real references rendered by the gallery:
    // addressable article naddr + showcase kind:1 nevent.
    let mut targets: Vec<SmokeTarget> = Vec::new();
    for (label, uri) in [
        (
            "embed_article (kind:30023 naddr)",
            nmp_gallery_tui::data::article_naddr().to_string(),
        ),
        (
            "embed_note (kind:1 nevent)",
            nmp_gallery_tui::data::note_nevent().to_string(),
        ),
    ] {
        match primary_id_for(&uri) {
            Some(primary_id) => targets.push(SmokeTarget {
                label,
                uri,
                primary_id,
            }),
            None => {
                eprintln!("smoke: could not decode URI for {label}: {uri}");
                return 1;
            }
        }
    }

    let consumer_id = "nmp-gallery-tui.smoke";

    println!("== nmp-gallery-tui --smoke ==");
    println!("kernel up, relays seeded; validating renderer-triggered embed claims.");
    println!();

    println!(
        "Target {} embed URI(s); waiting for relay connection then claiming:",
        targets.len()
    );
    for t in &targets {
        println!("  target: {} → {}", t.label, t.uri);
        println!(
            "    primary_id expected in claimed_events: {}",
            t.primary_id
        );
    }
    println!();

    let started = Instant::now();
    let mut claims_issued = false;
    let mut snapshot_tick = 0u32;
    let mut resolved_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    while started.elapsed() < timeout && resolved_ids.len() < targets.len() {
        let remaining = timeout - started.elapsed();
        match snapshot_rx.recv_timeout(remaining) {
            Ok(payload) => {
                let Some(value) = parse_snapshot(&payload) else {
                    continue;
                };
                snapshot_tick += 1;
                host.update_from_snapshot(&value);

                // Re-claim on EVERY snapshot tick until claims_issued.
                // The kernel's claim_event no-ops when !relays_ready
                // (W1 open-Q #3), so we keep trying until at least one
                // relay is connected — at which point the OneshotApi
                // interest registers and the planner compiles a wire REQ.
                if !claims_issued && any_relay_connected(&value) {
                    println!("  + relay connected — claims firing on tick #{snapshot_tick}");
                    for t in &targets {
                        println!("    claim: {}", t.uri);
                        sink.claim(&t.uri, consumer_id);
                    }
                    claims_issued = true;
                }

                // Print any target that just resolved.
                for t in &targets {
                    if resolved_ids.contains(&t.primary_id) {
                        continue;
                    }
                    if let Some(envelope) = host.current_envelopes().get(&t.primary_id) {
                        println!(
                            "+ resolved at {:.2}s: {}",
                            started.elapsed().as_secs_f32(),
                            t.label
                        );
                        print_resolved(t.label, envelope);
                        resolved_ids.insert(t.primary_id.clone());
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("snapshot channel disconnected before targets resolved");
                return 1;
            }
        }
    }

    println!();
    println!("Summary:");
    println!("  snapshot ticks observed: {snapshot_tick}");
    println!(
        "  claims issued:           {}",
        if claims_issued { "yes" } else { "no" }
    );
    println!(
        "  resolved targets:        {}/{}",
        resolved_ids.len(),
        targets.len()
    );
    let unresolved: Vec<&SmokeTarget> = targets
        .iter()
        .filter(|t| !resolved_ids.contains(&t.primary_id))
        .collect();
    println!();
    if unresolved.is_empty() {
        println!(
            "✅ ALL {} embed targets resolved in {:.2}s",
            targets.len(),
            started.elapsed().as_secs_f32()
        );
        0
    } else {
        println!(
            "❌ {}/{} targets unresolved after {:.2}s:",
            unresolved.len(),
            targets.len(),
            started.elapsed().as_secs_f32()
        );
        for t in &unresolved {
            println!(
                "  unresolved: {} → {} (expected primary_id: {})",
                t.label, t.uri, t.primary_id
            );
        }
        println!();
        println!("  Most likely cause: the target event isn't on the seeded relays.");
        println!("  The seeded relays are purplepag.es (indexer), nos.lol, relay.damus.io,");
        println!("  relay.nostr.band. Architecture is validated by the resolved targets above.");
        println!();
        println!(
            "Host envelope map ({} entries):",
            host.current_envelopes().len()
        );
        for (k, env) in host.current_envelopes() {
            println!("  - {k} → {}", projection_label(&env.projection));
        }
        1
    }
}

fn any_relay_connected(snapshot: &Value) -> bool {
    relay_status_array(snapshot)
        .map(|relays| {
            relays
                .iter()
                .any(|r| r.get("connection").and_then(Value::as_str) == Some("connected"))
        })
        .unwrap_or(false)
}

fn relay_status_array(snapshot: &Value) -> Option<&Vec<Value>> {
    snapshot
        .get("relay_statuses")
        .and_then(Value::as_array)
        .or_else(|| {
            snapshot
                .get("projections")
                .and_then(|p| p.get("relay_diagnostics"))
                .and_then(|d| d.get("relays"))
                .and_then(Value::as_array)
        })
        .or_else(|| snapshot.get("relay_status").and_then(Value::as_array))
}

fn projection_label(p: &nmp_content::embed_projection::EmbedKindProjection) -> &'static str {
    use nmp_content::embed_projection::EmbedKindProjection;
    match p {
        EmbedKindProjection::Article(_) => "Article (kind:30023)",
        EmbedKindProjection::ShortNote(_) => "ShortNote (kind:1)",
        EmbedKindProjection::Highlight(_) => "Highlight (kind:9802)",
        EmbedKindProjection::Profile(_) => "Profile (kind:0)",
        EmbedKindProjection::Unknown(_) => "Unknown",
    }
}

fn print_resolved(label: &str, env: &nmp_content::embed_projection::EmbeddedEventEnvelope) {
    use nmp_content::embed_projection::EmbedKindProjection;
    match &env.projection {
        EmbedKindProjection::Article(a) => {
            println!("✓ {label} → ArticleProjection (kind:30023)");
            println!("    id:        {}", a.id);
            println!("    author:    {}", a.author_pubkey);
            println!("    d_tag:     {}", a.d_tag);
            if let Some(title) = &a.title {
                println!("    title:     {title}");
            }
            if let Some(summary) = &a.summary {
                println!("    summary:   {summary}");
            }
            if let Some(hero) = &a.hero_image_url {
                println!("    hero:      {hero}");
            }
        }
        EmbedKindProjection::ShortNote(n) => {
            println!("✓ {label} → ShortNoteProjection (kind:1)");
            println!("    id:        {}", n.id);
            println!("    author:    {}", n.author_pubkey);
            println!("    media:     {:?}", n.media_urls);
        }
        EmbedKindProjection::Highlight(h) => {
            println!("✓ {label} → HighlightProjection (kind:9802)");
            println!("    id:        {}", h.id);
            println!(
                "    quoted:    {}",
                truncate_for_display(&h.highlighted_text, 80)
            );
        }
        EmbedKindProjection::Profile(p) => {
            println!("✓ {label} → ProfileProjection (kind:0)");
            println!("    pubkey:    {}", p.pubkey);
        }
        EmbedKindProjection::Unknown(u) => {
            println!("✓ {label} → UnknownProjection (kind:{})", u.kind);
            println!("    content:   {}", truncate_for_display(&u.content, 80));
        }
    }
}

fn truncate_for_display(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
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
    snapshot_rx: std::sync::mpsc::Receiver<String>,
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
    snapshot_rx: std::sync::mpsc::Receiver<String>,
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
                let new_authors = host.update_from_snapshot(&snapshot);
                live_profiles.update_from_snapshot(&snapshot);
                claim_profiles_for(sink, &new_authors);
                // Coalesce any additional snapshots that have already piled
                // up so we don't redraw N times for N quick ticks. Latest
                // wins (the host replaces its state from each tick).
                while let Ok(extra) = rx.try_recv() {
                    match extra {
                        GalleryEvent::Snapshot(next) => {
                            let more = host.update_from_snapshot(&next);
                            live_profiles.update_from_snapshot(&next);
                            claim_profiles_for(sink, &more);
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

/// Fire `claim_profile` for each author whose kind:0 hasn't arrived in
/// `claimed_events.author_display_name` yet. `update_from_snapshot`
/// returns the deduped pubkey list each tick; we let the kernel's
/// per-(pubkey, consumer_id) refcounting dedup across ticks — re-claiming
/// the same author on every snapshot is a near-no-op once kind:0 is
/// cached. Component composability: the article renderer reads the
/// enriched `ArticleProjection.author_display_name` and composes with
/// `NostrProfileName`, falling back to truncated npub while in-flight.
fn claim_profiles_for(sink: &Arc<LiveKernelSink>, authors: &[String]) {
    for pubkey in authors {
        sink.claim_profile(pubkey, "nmp-gallery-tui.embed.author");
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

fn spawn_snapshot_thread(tx: Sender<GalleryEvent>, rx: std::sync::mpsc::Receiver<String>) {
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

#[derive(Default)]
struct VisibleProfileClaims {
    active: BTreeSet<(String, String)>,
}

impl VisibleProfileClaims {
    fn reconcile(&mut self, sink: &LiveKernelSink, current: BTreeSet<(String, String)>) {
        for (pubkey, consumer_id) in self.active.difference(&current) {
            sink.release_profile(pubkey, consumer_id);
        }
        self.active = current;
    }
}
