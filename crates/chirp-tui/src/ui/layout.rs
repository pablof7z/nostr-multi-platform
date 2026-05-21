use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{AppState, Pane};

pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    render_title(frame, rows[0], state);
    render_body(frame, rows[1], state);
    render_compose(frame, rows[2]);
    render_status(frame, rows[3], state);
}

fn render_title(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let title = Line::from(vec![
        Span::styled(
            "chirp",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("[{}]", state.tab),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(" [mentions] [dms] [groups]"),
    ]);
    frame.render_widget(Paragraph::new(title), area);
}

fn render_body(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(28),
            Constraint::Percentage(44),
            Constraint::Percentage(28),
        ])
        .split(area);

    let feed = Paragraph::new(vec![
        Line::from("Home feed"),
        Line::from(format!("cards: {}  blocks: {}", state.cards, state.blocks)),
        Line::from("Updates arrive from the kernel callback."),
    ])
    .block(panel("Feed", state.focused == Pane::Feed))
    .wrap(Wrap { trim: true });
    frame.render_widget(feed, panes[0]);

    let detail = Paragraph::new(vec![
        Line::from("Note / Thread"),
        Line::from("Thread rendering lands after feed selection."),
    ])
    .block(panel("Detail", state.focused == Pane::Detail))
    .wrap(Wrap { trim: true });
    frame.render_widget(detail, panes[1]);

    let profile = Paragraph::new(vec![
        Line::from("Profile / Detail"),
        Line::from("Profile opens once feed selection exists."),
    ])
    .block(panel("Profile", state.focused == Pane::Profile))
    .wrap(Wrap { trim: true });
    frame.render_widget(profile, panes[2]);
}

fn render_compose(frame: &mut Frame<'_>, area: Rect) {
    let compose = Paragraph::new("i compose  r reply  + react  / command")
        .block(Block::default().borders(Borders::ALL).title("Compose"));
    frame.render_widget(compose, area);
}

fn render_status(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let status = format!(
        "{}  updates:{}  q quit  1/2/3 focus",
        state.status, state.update_count
    );
    frame.render_widget(Paragraph::new(status), area);
}

fn panel(title: &'static str, focused: bool) -> Block<'static> {
    let style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(style)
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    #[test]
    fn renders_three_pane_skeleton_at_120_by_40() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = AppState::default();

        terminal.draw(|frame| render(frame, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        let rendered = format!("{buffer:?}");

        assert!(rendered.contains("chirp"));
        assert!(rendered.contains("Feed"));
        assert!(rendered.contains("Detail"));
        assert!(rendered.contains("Profile"));
        assert!(rendered.contains("Compose"));
    }
}
