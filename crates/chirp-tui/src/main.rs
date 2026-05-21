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
use chirp_tui::render_intents::{RenderIntent, RenderIntentDiff, RenderIntentTracker};
use chirp_tui::ui;

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
    for relay in &args.relays {
        runtime
            .add_relay(relay, "both,indexer")
            .map_err(|e| eyre!(e))?;
    }

    let (ui_tx, ui_rx) = mpsc::channel();
    spawn_terminal_reader(ui_tx.clone());
    spawn_nmp_forwarder(nmp_rx, ui_tx);

    let mut state = AppState::default();
    let mut render_intents = RenderIntentTracker::default();
    if args.basic {
        state.set_basic();
    }

    terminal.draw(|frame| ui::layout::render(frame, &state))?;

    while let Ok(event) = ui_rx.recv() {
        match event {
            UiEvent::Terminal(Event::Key(key)) => {
                if input::handle_key(&mut state, &runtime, key) == InputFlow::Quit {
                    break;
                }
            }
            UiEvent::Terminal(_) => {}
            UiEvent::Nmp(event) => state.apply_nmp_event(&runtime, event),
        }
        let diff = render_intents.sync_rows(&state.rows);
        apply_render_intents(&runtime, diff).map_err(|e| eyre!(e))?;
        terminal.draw(|frame| ui::layout::render(frame, &state))?;
    }

    Ok(())
}

fn apply_render_intents(runtime: &AppRuntime, diff: RenderIntentDiff) -> chirp_tui::Result<()> {
    for intent in diff.removed {
        if let RenderIntent::AuthorProfile { pubkey } = intent {
            runtime.release_visible_author_profile(&pubkey)?;
        }
    }
    for intent in diff.added {
        if let RenderIntent::AuthorProfile { pubkey } = intent {
            runtime.claim_visible_author_profile(&pubkey)?;
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
