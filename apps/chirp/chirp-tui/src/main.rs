use std::sync::mpsc;
use std::thread;

use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use chirp_tui::app::{AppRuntime, AppState};
use chirp_tui::bridge::NmpEvent;
use chirp_tui::input::{self, InputFlow};
use chirp_tui::media_cache::{visible_media_urls, MediaCache, MediaFetch};
use chirp_tui::render_intents::{RenderIntent, RenderIntentDiff, RenderIntentTracker};
use chirp_tui::ui::{self, layout::RenderContext};

#[derive(Debug, Parser)]
#[command(
    name = "chirp-tui",
    about = "Terminal shell for the Chirp Nostr client"
)]
struct Args {
    #[arg(long)]
    basic: bool,

    #[arg(long = "relay")]
    relays: Vec<String>,
}

enum UiEvent {
    Terminal(Event),
    Nmp(NmpEvent),
    Media(MediaFetch),
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    run(args)
}

fn run(args: Args) -> Result<()> {
    let _terminal = TerminalGuard::enter()?;
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (runtime, nmp_rx) = AppRuntime::new().map_err(|e| eyre!(e))?;
    if args.relays.is_empty() {
        for entry in nmp_chirp_config::chirp_default_relay_bootstrap() {
            runtime
                .add_relay(entry.url, entry.role)
                .map_err(|e| eyre!(e))?;
        }
    } else {
        for relay in &args.relays {
            runtime
                .add_relay(relay, "both,indexer")
                .map_err(|e| eyre!(e))?;
        }
    }

    let (ui_tx, ui_rx) = mpsc::channel();
    let (media_tx, media_rx) = mpsc::channel();
    spawn_terminal_reader(ui_tx.clone());
    spawn_nmp_forwarder(nmp_rx, ui_tx.clone());
    spawn_media_forwarder(media_rx, ui_tx);

    let mut state = AppState::default();
    let mut media_cache = MediaCache::new();
    let mut render_intents = RenderIntentTracker::default();
    if args.basic {
        state.set_basic();
    }

    draw(&mut terminal, &state, &media_cache)?;

    while let Ok(event) = ui_rx.recv() {
        match event {
            UiEvent::Terminal(Event::Key(key)) => {
                if input::handle_key(&mut state, &runtime, key) == InputFlow::Quit {
                    break;
                }
            }
            UiEvent::Terminal(_) => {}
            UiEvent::Nmp(event) => state.apply_nmp_event(&runtime, event),
            UiEvent::Media(event) => media_cache.apply_fetch(event),
        }
        let diff = render_intents.sync_rows(state.render_intent_rows());
        apply_render_intents(&runtime, diff).map_err(|e| eyre!(e))?;
        media_cache.sync_urls(visible_media_urls(&state), media_tx.clone());
        draw(&mut terminal, &state, &media_cache)?;
    }

    Ok(())
}

fn draw(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &AppState,
    media_cache: &MediaCache,
) -> Result<()> {
    let media_images = media_cache.image_refs();
    terminal.draw(|frame| {
        ui::layout::render_with_context(
            frame,
            state,
            &RenderContext {
                media_images: &media_images,
            },
        )
    })?;
    Ok(())
}

fn apply_render_intents(runtime: &AppRuntime, diff: RenderIntentDiff) -> chirp_tui::Result<()> {
    for intent in diff.removed {
        match intent {
            RenderIntent::AuthorProfile { pubkey } => {
                runtime.release_visible_author_profile(&pubkey)?;
            }
            RenderIntent::NoteRelations { event_id } => {
                runtime.release_visible_note_relation_counts(&event_id)?;
            }
        }
    }
    for intent in diff.added {
        match intent {
            RenderIntent::AuthorProfile { pubkey } => {
                runtime.claim_visible_author_profile(&pubkey)?;
            }
            RenderIntent::NoteRelations { event_id } => {
                runtime.claim_visible_note_relation_counts(&event_id)?;
            }
        }
    }
    Ok(())
}

fn spawn_terminal_reader(tx: mpsc::Sender<UiEvent>) {
    thread::spawn(move || {
        while let Ok(event) = event::read() {
            if tx.send(UiEvent::Terminal(event)).is_err() {
                break;
            }
        }
    });
}

fn spawn_nmp_forwarder(rx: mpsc::Receiver<NmpEvent>, tx: mpsc::Sender<UiEvent>) {
    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            if tx.send(UiEvent::Nmp(event)).is_err() {
                break;
            }
        }
    });
}

fn spawn_media_forwarder(rx: mpsc::Receiver<MediaFetch>, tx: mpsc::Sender<UiEvent>) {
    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            if tx.send(UiEvent::Media(event)).is_err() {
                break;
            }
        }
    });
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(std::io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
    }
}
