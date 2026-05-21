use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{AppState, Mode, Pane};
use crate::timeline::TimelineRow;

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
    render_compose(frame, rows[2], state);
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

    let feed = Paragraph::new(feed_lines(
        state,
        panes[0].height.saturating_sub(2) as usize,
    ))
    .block(panel("Feed", state.focused == Pane::Feed))
    .wrap(Wrap { trim: true });
    frame.render_widget(feed, panes[0]);

    let detail = Paragraph::new(detail_lines(state))
        .block(panel("Detail", state.focused == Pane::Detail))
        .wrap(Wrap { trim: true });
    frame.render_widget(detail, panes[1]);

    let profile = Paragraph::new(profile_lines(state))
        .block(panel("Profile", state.focused == Pane::Profile))
        .wrap(Wrap { trim: true });
    frame.render_widget(profile, panes[2]);
}

fn feed_lines(state: &AppState, height: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(format!(
        "cards: {}  blocks: {}",
        state.cards, state.blocks
    ))];
    if state.rows.is_empty() {
        lines.push(Line::from("Waiting for timeline events..."));
        return lines;
    }

    let visible = height.saturating_sub(1).max(1);
    let start = state.selected.saturating_sub(visible.saturating_sub(1) / 2);
    for (idx, row) in state.rows.iter().enumerate().skip(start).take(visible) {
        lines.push(render_feed_row(row, idx == state.selected));
    }
    lines
}

fn render_feed_row(row: &TimelineRow, selected: bool) -> Line<'static> {
    let prefix = if selected { ">" } else { " " };
    let indent = "  ".repeat(row.depth.min(3));
    let gap = if row.has_gap { "*" } else { " " };
    let text = format!(
        "{prefix}{indent}{gap} {}  {}",
        row.author,
        row.content.replace('\n', " ")
    );
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::from(Span::styled(text, style))
}

fn detail_lines(state: &AppState) -> Vec<Line<'static>> {
    let Some(row) = state.selected_row() else {
        return vec![
            Line::from("Note / Thread"),
            Line::from("Select a feed row once events arrive."),
        ];
    };
    vec![
        Line::from(row.author.clone()),
        Line::from(format!("event {}", short_id(&row.id))),
        Line::from(row.content.clone()),
        Line::from("Enter opens the full thread through NMP."),
    ]
}

fn profile_lines(state: &AppState) -> Vec<Line<'static>> {
    let Some(row) = state.selected_row() else {
        return vec![Line::from("Profile / Detail")];
    };
    vec![
        Line::from("Selected author"),
        Line::from(row.author.clone()),
        Line::from("p opens the full profile through NMP."),
    ]
}

fn short_id(value: &str) -> String {
    if value.len() <= 16 {
        value.to_string()
    } else {
        format!("{}...{}", &value[..8], &value[value.len() - 6..])
    }
}

fn render_compose(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let (title, body) = if state.mode == Mode::Compose {
        let target = state.reply_to.as_deref().map_or("new note", |_| "reply");
        let text = if state.compose.is_empty() {
            format!("{target}: ")
        } else {
            format!("{target}: {}", state.compose)
        };
        (
            format!("Compose ({})", state.compose.chars().count()),
            format!("{text}\nCtrl+Enter publish  Esc cancel"),
        )
    } else {
        (
            "Compose".to_string(),
            "i compose  r reply  + react  f follow  F unfollow".to_string(),
        )
    };
    let compose = Paragraph::new(body).block(Block::default().borders(Borders::ALL).title(title));
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

    #[test]
    fn renders_feed_rows_from_state() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.rows.push(TimelineRow {
            id: "event-1".to_string(),
            author: "alice".to_string(),
            author_pubkey: "alice-pubkey".to_string(),
            content: "hello from nostr".to_string(),
            created_at: 1,
            depth: 0,
            has_gap: false,
        });

        terminal.draw(|frame| render(frame, &state)).unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());

        assert!(rendered.contains("alice"));
        assert!(rendered.contains("hello from nostr"));
    }
}
