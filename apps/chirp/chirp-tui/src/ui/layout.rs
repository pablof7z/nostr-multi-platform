use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AppState;
use crate::app::Mode;
use crate::features::FeatureTab;
use crate::ui::feature_panels;
use crate::ui::help;
use crate::ui::home;

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
    match state.tab {
        FeatureTab::Home => home::render(frame, area, state),
        _ => feature_panels::render(frame, area, state),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_line_clears_or_truncates_to_width() {
        assert_eq!(fit_line("abc".to_string(), 5), "abc  ");
        assert_eq!(fit_line("abcdef".to_string(), 4), "abcd");
    }
}
