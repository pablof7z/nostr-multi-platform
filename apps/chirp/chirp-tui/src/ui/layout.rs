use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui_image::protocol::Protocol;

use crate::app::AppState;
use crate::app::Mode;
use crate::features::FeatureTab;
use crate::short_id;
use crate::ui::colors::{ACCENT_CYAN, BODY_TEXT, DIM_TEXT, DIMMER_TEXT, RELAY_DOWN, RELAY_OK};
use crate::ui::feature_panels;
use crate::ui::help;
use crate::ui::home;
use crate::ui::raw_event_modal;

pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    render_with_context(frame, state, &RenderContext::empty());
}

pub struct RenderContext<'a> {
    pub media_images: &'a [(&'a str, &'a Protocol)],
}

impl<'a> RenderContext<'a> {
    pub const fn empty() -> Self {
        Self { media_images: &[] }
    }
}

pub fn render_with_context(frame: &mut Frame<'_>, state: &AppState, context: &RenderContext<'_>) {
    let area = frame.area();

    // First-run welcome: no accounts configured yet.
    if state.features.accounts.is_empty() {
        render_welcome(frame, area, state);
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(8),    // body
            Constraint::Length(3), // compose / input bar
            Constraint::Length(1), // status
        ])
        .split(area);

    render_title(frame, rows[0], state);
    render_body(frame, rows[1], state, context);
    render_compose(frame, rows[2], state);
    render_status(frame, rows[3], state);

    if state.show_help {
        help::render_with_state(frame, area, state);
    }

    if state.mode == Mode::Compose {
        render_compose_modal(frame, area, state);
    }

    if let Mode::RawEventModal { scroll } = state.mode {
        raw_event_modal::render(frame, area, state, scroll);
    }
}

fn render_welcome(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(3)])
        .split(area);

    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "chirp",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "the nostr social client",
            Style::default().fg(DIM_TEXT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "n  import nsec    c  create account    ?  help    q  quit",
            Style::default().fg(DIM_TEXT),
        )),
    ];
    let welcome = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(welcome, rows[0]);

    // Render input bar at bottom when n is pressed.
    render_compose(frame, rows[1], state);

    if state.show_help {
        help::render_with_state(frame, area, state);
    }
}

fn render_title(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let account = state
        .features
        .accounts
        .iter()
        .find(|a| a.active)
        .map(|a| format!("@{}", a.display))
        .unwrap_or_default();

    let connected = state
        .relays
        .iter()
        .filter(|r| {
            let lower = r.connection_label.to_ascii_lowercase();
            lower.contains("connected") || lower == "open"
        })
        .count();
    let relay_dot = if connected > 0 { '\u{25cf}' } else { '\u{25cb}' };
    let relay_color = if connected > 0 { RELAY_OK } else { RELAY_DOWN };

    let title = Line::from(vec![
        Span::styled(
            "chirp",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(account, Style::default().fg(DIM_TEXT)),
        Span::raw("  "),
        Span::raw(tab_labels(state)),
        Span::raw("  "),
        Span::styled(
            format!("{} {} relays", relay_dot, state.relays.len()),
            Style::default().fg(relay_color),
        ),
    ]);
    frame.render_widget(Paragraph::new(title), area);
}

fn render_body(frame: &mut Frame<'_>, area: Rect, state: &AppState, context: &RenderContext<'_>) {
    match state.tab {
        FeatureTab::Home => home::render(frame, area, state, context),
        _ => feature_panels::render(frame, area, state),
    }
}

fn render_compose(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let (title, body) = if state.mode == Mode::Command {
        (
            "Command".to_string(),
            format!(":{}\nEnter run  Esc cancel", state.command),
        )
    } else if state.mode == Mode::InputBar {
        let label = if state.input_bar_label.is_empty() {
            "input".to_string()
        } else {
            state.input_bar_label.clone()
        };
        let display = if state.input_bar_masked {
            "\u{25cf}".repeat(state.input_bar_value.chars().count())
        } else {
            state.input_bar_value.clone()
        };
        (
            label,
            format!("{}\u{2588}\nEnter confirm  Esc cancel", display),
        )
    } else if state.mode == Mode::ModalForm {
        let fields: String = state
            .modal_fields
            .iter()
            .enumerate()
            .map(|(i, (l, v))| {
                if i == state.modal_cursor {
                    format!("{}: {}\u{2588}", l, v)
                } else {
                    format!("{}: {}", l, v)
                }
            })
            .collect::<Vec<_>>()
            .join("  \u{2502}  ");
        (
            state.modal_title.clone(),
            format!("{}\nTab next  Enter submit  Esc cancel", fields),
        )
    } else if state.mode == Mode::AccountSwitcher {
        let accounts = state
            .features
            .accounts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if i == state.account_switcher_cursor {
                    format!("[{}]", a.display)
                } else {
                    a.display.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        (
            "Switch Account".to_string(),
            format!("{}\nj/k move  Enter switch  Esc cancel", accounts),
        )
    } else {
        (
            "Compose".to_string(),
            "n new  r reply  + react  z zap  f follow  / palette  a accounts  ? help  q quit"
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

fn render_compose_modal(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let width = ((area.width as u32 * 70 / 100) as u16).max(52);
    let height = 14u16.min(area.height.saturating_sub(4));
    let popup = centered_rect(area, width, height);

    frame.render_widget(Clear, popup);

    let title = match state.reply_to.as_deref() {
        Some(target) => format!(" \u{21a9} Reply to {} ", short_id(target)),
        None => " \u{270f} New Note ".to_string(),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(ACCENT_CYAN));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let inner_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let mut compose_text = state.compose.clone();
    compose_text.push('\u{2588}');
    let text_area = Paragraph::new(compose_text)
        .style(Style::default().fg(BODY_TEXT))
        .wrap(Wrap { trim: false });
    frame.render_widget(text_area, inner_rows[0]);

    let char_count = state.compose.chars().count();
    let hint_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(12)])
        .split(inner_rows[1]);

    let key_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(DIM_TEXT);
    let hint_line = Line::from(vec![
        Span::styled("Enter", key_style),
        Span::styled(" send  ", desc_style),
        Span::styled("Shift+Enter", key_style),
        Span::styled(" newline  ", desc_style),
        Span::styled("Esc", key_style),
        Span::styled(" cancel", desc_style),
    ]);
    frame.render_widget(Paragraph::new(hint_line), hint_cols[0]);

    let count_line = Line::from(Span::styled(
        format!("{char_count} chars"),
        Style::default().fg(DIMMER_TEXT),
    ));
    frame.render_widget(
        Paragraph::new(count_line).alignment(Alignment::Right),
        hint_cols[1],
    );
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(h) / 2),
            Constraint::Length(h),
            Constraint::Min(0),
        ])
        .split(area);
    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(w) / 2),
            Constraint::Length(w),
            Constraint::Min(0),
        ])
        .split(vert[1]);
    horiz[1]
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
