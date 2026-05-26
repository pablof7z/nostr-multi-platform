use std::{io, sync::mpsc, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use nmp_gallery_tui::{data::GalleryData, gallery, render};
use ratatui::{backend::CrosstermBackend, Terminal};

struct Args {
    component: String,
    dump_lines: bool,
    hold_ms: Option<u64>,
    list: bool,
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

    let data = match GalleryData::load(!args.dump_lines) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("failed to load NmpGallery data: {error}");
            std::process::exit(1);
        }
    };
    if args.dump_lines {
        for line in render::plain_lines(&args.component, &data, 96) {
            println!("{line}");
        }
        return Ok(());
    }

    run_terminal(&args, &data)
}

fn parse_args() -> Args {
    let mut component = "content-view".to_string();
    let mut dump_lines = false;
    let mut hold_ms = None;
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
            "--hold-ms" => {
                hold_ms = iter.next().and_then(|value| value.parse::<u64>().ok());
            }
            "--list" => list = true,
            value if !value.starts_with('-') => component = value.to_string(),
            _ => {}
        }
    }

    Args {
        component,
        dump_lines,
        hold_ms,
        list,
    }
}

fn run_terminal(args: &Args, data: &GalleryData) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = draw_and_wait(&mut terminal, args, data);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn draw_and_wait(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    args: &Args,
    data: &GalleryData,
) -> io::Result<()> {
    let mut selected_index = gallery::component_index(&args.component);
    draw_gallery(terminal, selected_index, data)?;

    if let Some(ms) = args.hold_ms {
        hold_for(Duration::from_millis(ms));
        return Ok(());
    }

    loop {
        match event::read()? {
            Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) => {
                return Ok(())
            }
            Event::Key(key) if matches!(key.code, KeyCode::Down | KeyCode::Char('j')) => {
                let count = gallery::component_count().max(1);
                selected_index = (selected_index + 1) % count;
                draw_gallery(terminal, selected_index, data)?;
            }
            Event::Key(key) if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) => {
                let count = gallery::component_count().max(1);
                selected_index = (selected_index + count - 1) % count;
                draw_gallery(terminal, selected_index, data)?;
            }
            Event::Key(key) if matches!(key.code, KeyCode::Home) => {
                selected_index = 0;
                draw_gallery(terminal, selected_index, data)?;
            }
            Event::Key(key) if matches!(key.code, KeyCode::End) => {
                selected_index = gallery::component_count().saturating_sub(1);
                draw_gallery(terminal, selected_index, data)?;
            }
            Event::Resize(_, _) => {
                draw_gallery(terminal, selected_index, data)?;
            }
            _ => {}
        }
    }
}

fn hold_for(duration: Duration) {
    let (_sender, receiver) = mpsc::channel::<()>();
    let _ = receiver.recv_timeout(duration);
}

fn draw_gallery(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    selected_index: usize,
    data: &GalleryData,
) -> io::Result<()> {
    terminal.draw(|frame| {
        frame.render_widget(
            gallery::GalleryView::new(selected_index, data),
            frame.area(),
        )
    })?;
    Ok(())
}
