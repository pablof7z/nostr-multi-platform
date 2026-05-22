use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{AppState, Mode, Pane};
use crate::features::FeatureTab;
use crate::timeline::TimelineRow;
use crate::ui::feature_panels;
use crate::ui::help;
use crate::ui::shared_snapshot_lines::{action_summary, relay_lines};

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
    if state.show_help {
        help::render(frame, area);
    }
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
            format!("[{}]", state.tab.label()),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::raw(tab_labels(state)),
    ]);
    frame.render_widget(Paragraph::new(title), area);
}

fn render_body(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    if state.tab != FeatureTab::Home {
        feature_panels::render(frame, area, state);
        return;
    }

    if state.basic || area.width < 80 {
        render_feed_panel(frame, area, state);
        return;
    }

    if area.width < 104 {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);
        render_feed_panel(frame, panes[0], state);
        render_detail_panel(frame, panes[1], state);
        return;
    }

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(28),
            Constraint::Percentage(44),
            Constraint::Percentage(28),
        ])
        .split(area);

    render_feed_panel(frame, panes[0], state);
    render_detail_panel(frame, panes[1], state);
    render_profile_panel(frame, panes[2], state);
}

fn render_feed_panel(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let feed = Paragraph::new(feed_lines(state, area.height.saturating_sub(2) as usize))
        .block(panel("Feed", state.focused == Pane::Feed))
        .wrap(Wrap { trim: true });
    frame.render_widget(feed, area);
}

fn render_detail_panel(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let detail = Paragraph::new(detail_lines(state))
        .block(panel("Detail", state.focused == Pane::Detail))
        .wrap(Wrap { trim: true });
    frame.render_widget(detail, area);
}

fn render_profile_panel(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let profile = Paragraph::new(profile_lines(state))
        .block(panel("Profile", state.focused == Pane::Profile))
        .wrap(Wrap { trim: true });
    frame.render_widget(profile, area);
}

fn feed_lines(state: &AppState, height: usize) -> Vec<Line<'static>> {
    let item_count = state.rows.len();
    let selected = if item_count == 0 {
        "0/0".to_string()
    } else {
        format!("{}/{}", state.selected + 1, item_count)
    };
    let mut lines = vec![Line::from(format!(
        "items: {selected}  cards: {}  blocks: {}  events_rx: {}",
        state.cards, state.blocks, state.metrics.events_rx
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
        "{prefix}{indent}{gap} {}  {}  [{}]",
        row.author,
        row.content.replace('\n', " "),
        row.relation_counts.summary()
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
        Line::from(row.relation_counts.summary()),
        Line::from(row.content.clone()),
        Line::from(format!(
            "visible {}  queue {}  seq {}",
            state.metrics.visible_items,
            state.metrics.actor_queue_depth,
            state.metrics.update_sequence
        )),
        Line::from(action_summary(state)),
        Line::from("Enter opens the full thread through NMP."),
    ]
}

fn profile_lines(state: &AppState) -> Vec<Line<'static>> {
    if let Some(profile) = &state.features.author_profile {
        let mut lines = vec![
            Line::from(profile.display.clone()),
            Line::from(profile.note_count.clone()),
            Line::from(profile.about.clone()),
        ];
        if !profile.action_label.is_empty() {
            lines.push(Line::from(format!("action: {}", profile.action_label)));
        }
        lines.extend(relay_lines(state));
        return lines;
    }

    let Some(row) = state.selected_row() else {
        return relay_lines(state);
    };
    let mut lines = vec![
        Line::from("Selected author"),
        Line::from(row.author.clone()),
        Line::from("p opens the full profile through NMP."),
        Line::from(""),
    ];
    lines.extend(relay_lines(state));
    lines
}

fn short_id(value: &str) -> String {
    if value.len() <= 16 {
        value.to_string()
    } else {
        format!("{}...{}", &value[..8], &value[value.len() - 6..])
    }
}

fn render_compose(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let (title, body) = if state.mode == Mode::Command {
        (
            "Command".to_string(),
            format!(":{}\nEnter run  Esc cancel", state.command),
        )
    } else if state.mode == Mode::Compose {
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
            "h/c/g/w/s tabs  : command  i compose  r reply  + react  f/F follow  ? help"
                .to_string(),
        )
    };
    let compose = Paragraph::new(body).block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(compose, area);
}

fn tab_labels(state: &AppState) -> String {
    if state.basic {
        return "[basic]".to_string();
    }
    crate::features::FeatureTab::ALL
        .iter()
        .map(|tab| {
            if *tab == state.tab {
                format!("[{}]", tab.label())
            } else {
                tab.label().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_status(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let status = format!(
        "{}  updates:{}  pending:{}  q quit  ? help  1/2/3 focus",
        state.status,
        state.update_count,
        state.pending_actions.len()
    );
    frame.render_widget(Paragraph::new(fit_line(status, area.width as usize)), area);
}

fn fit_line(text: String, width: usize) -> String {
    let mut fitted = text.chars().take(width).collect::<String>();
    let len = fitted.chars().count();
    if len < width {
        fitted.push_str(&" ".repeat(width - len));
    }
    fitted
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
    use super::*;

    #[test]
    fn fit_line_clears_or_truncates_to_width() {
        assert_eq!(fit_line("abc".to_string(), 5), "abc  ");
        assert_eq!(fit_line("abcdef".to_string(), 4), "abcd");
    }
}
