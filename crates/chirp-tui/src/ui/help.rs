use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

pub fn render(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered(area, 62, 16);
    frame.render_widget(Clear, popup);
    frame.render_widget(help_panel(), popup);
}

fn help_panel() -> Paragraph<'static> {
    let lines = vec![
        Line::from(vec![hotkey("j/k"), text(" or arrows move selection")]),
        Line::from(vec![hotkey("PgUp/PgDn"), text(" page feed")]),
        Line::from(vec![hotkey("Home/End"), text(" jump to top/bottom")]),
        Line::from(vec![hotkey("1 2 3"), text(" focus feed/detail/profile")]),
        Line::from(vec![hotkey("Enter"), text(" open selected thread")]),
        Line::from(vec![hotkey("p"), text(" open selected author")]),
        Line::from(vec![hotkey("i"), text(" compose note")]),
        Line::from(vec![hotkey("r"), text(" reply to selected note")]),
        Line::from(vec![hotkey("+"), text(" react; f/F follow/unfollow")]),
        Line::from(vec![hotkey("Ctrl+Enter"), text(" publish compose")]),
        Line::from(vec![hotkey("Esc"), text(" close help or cancel compose")]),
        Line::from(vec![hotkey("q"), text(" quit")]),
    ];
    Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .wrap(Wrap { trim: true })
}

fn hotkey(value: &'static str) -> Span<'static> {
    Span::styled(
        format!("{value:<12}"),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

fn text(value: &'static str) -> Span<'static> {
    Span::raw(value)
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(20);
    let height = height.min(area.height.saturating_sub(2)).max(8);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
}
