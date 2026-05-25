//! Approach-b Home: bottom-left relay health panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AppState;
use crate::snapshot::RelayRow;
use crate::ui::colors::{
    BODY_TEXT, DIM_TEXT, DIMMER_TEXT, LIST_BG, RELAY_CONNECTING, RELAY_DOWN, RELAY_OK,
};

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Relays ")
        .border_style(Style::default().fg(DIMMER_TEXT))
        .style(Style::default().bg(LIST_BG));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let lines = build_lines(&state.relays, pane_width);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

fn build_lines(relays: &[RelayRow], pane_width: usize) -> Vec<Line<'static>> {
    if relays.is_empty() {
        return vec![Line::from(Span::styled(
            "no relay diagnostics yet",
            Style::default().fg(DIM_TEXT),
        ))];
    }

    relays
        .iter()
        .take(8)
        .map(|relay| relay_line(relay, pane_width))
        .collect()
}

fn relay_line(relay: &RelayRow, pane_width: usize) -> Line<'static> {
    let (dot_char, dot_color) = status_dot(&relay.connection_label);
    let count = relay.total_events_display.clone();
    let count_len = count.chars().count();

    let dot_width = 2usize; // dot + space
    let max_url_width = pane_width
        .saturating_sub(dot_width)
        .saturating_sub(count_len)
        .saturating_sub(1);
    let url = truncate(&relay.short_url, max_url_width);
    let url_len = url.chars().count();
    let used = dot_width + url_len + 1 + count_len;
    let pad_len = pane_width.saturating_sub(used);
    let pad = if pad_len > 0 {
        " ".repeat(pad_len)
    } else {
        String::new()
    };

    Line::from(vec![
        Span::styled(format!("{} ", dot_char), Style::default().fg(dot_color)),
        Span::styled(url, Style::default().fg(BODY_TEXT)),
        Span::raw(" "),
        Span::styled(pad, Style::default()),
        Span::styled(count, Style::default().fg(DIM_TEXT)),
    ])
}

fn status_dot(connection_label: &str) -> (char, ratatui::style::Color) {
    let lower = connection_label.to_ascii_lowercase();
    if lower.contains("disconnected") || lower.contains("down") || lower.contains("failed") {
        ('\u{25cb}', RELAY_DOWN) // ○
    } else if lower.contains("connected") || lower == "open" {
        ('\u{25cf}', RELAY_OK) // ●
    } else {
        ('\u{25cc}', RELAY_CONNECTING) // ◌
    }
}

fn truncate(value: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = value.chars().count();
    if count <= max {
        value.to_string()
    } else if max <= 1 {
        value.chars().take(max).collect()
    } else {
        let mut out: String = value.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}
