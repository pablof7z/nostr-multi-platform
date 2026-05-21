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
    if args.basic {
        state.status = "basic mode: images and animations disabled".to_string();
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
        terminal.draw(|frame| ui::layout::render(frame, &state))?;
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
