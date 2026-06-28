//! Approach-b Home: bottom-left relay health panel.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::snapshot::RelayRow;
use crate::ui::colors::{
    BODY_TEXT, DIMMER_TEXT, DIM_TEXT, LIST_BG, RELAY_CONNECTING, RELAY_DOWN, RELAY_OK,
};

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let relay_count = state.relays.len();
    let title = if relay_count == 0 {
        " Relays ".to_string()
    } else {
        format!(" Relays {}/{} ", relay_count.min(8), relay_count)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
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
    let (dot_char, dot_color) = status_dot(&relay.connection);
    let count = compact_count(relay.total_events_rx);
    let count_len = count.chars().count();

    let dot_width = 2usize; // dot + space
    let max_url_width = pane_width
        .saturating_sub(dot_width)
        .saturating_sub(count_len)
        .saturating_sub(1);
    let url = truncate(&short_relay_url(&relay.relay_url), max_url_width);
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

/// Strip the `ws[s]://` scheme and trailing `/` for a compact relay label.
fn short_relay_url(url: &str) -> String {
    url.strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_string()
}

/// Compact integer formatter for event counts: `1234` → `"1.2K"`.
fn compact_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
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
