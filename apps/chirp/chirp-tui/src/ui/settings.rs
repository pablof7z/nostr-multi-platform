//! Approach-b Settings tab: 3-pane layout (Accounts | Relays | Outbox).

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AppState;
use crate::feature_snapshot::AccountLine;
use crate::snapshot::RelayRow;
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DIM_TEXT, DIMMER_TEXT, LIST_BG, RELAY_CONNECTING, RELAY_DOWN,
    RELAY_OK, SELECTED_BG, ZAP, author_color,
};
use crate::ui::outbox;
use crate::ui::shared_snapshot_lines::relay_lines;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(35),
            Constraint::Percentage(30),
        ])
        .split(area);

    render_accounts(frame, cols[0], state);
    render_relays(frame, cols[1], state);
    outbox::render_outbox(frame, cols[2], state);
}

fn render_accounts(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(LIST_BG))
        .title(Span::styled(
            " Accounts ",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let lines = if state.features.accounts.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No accounts configured",
                Style::default().fg(DIM_TEXT),
            )),
        ]
    } else {
        let mut all: Vec<Line<'static>> = Vec::new();
        for (i, account) in state.features.accounts.iter().enumerate() {
            let selected = i == state.settings_account_selected;
            append_account_card(&mut all, account, selected, pane_width);
        }
        all
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn append_account_card(
    lines: &mut Vec<Line<'static>>,
    account: &AccountLine,
    selected: bool,
    pane_width: usize,
) {
    let row_bg = if selected { SELECTED_BG } else { LIST_BG };
    let gutter = if selected {
        Span::styled("\u{2503} ", Style::default().fg(ACCENT_CYAN).bg(row_bg))
    } else {
        Span::styled("  ", Style::default().bg(row_bg))
    };
    let gutter_width = 2usize;
    let content_width = pane_width.saturating_sub(gutter_width);

    // Row 1: active marker + display name
    let active_marker = if account.active {
        Span::styled("* ", Style::default().fg(ZAP).bg(row_bg))
    } else {
        Span::styled("  ", Style::default().bg(row_bg))
    };
    let name_max = content_width.saturating_sub(2);
    let name = truncate(&account.display, name_max);
    let name_len = name.chars().count();
    let col = author_color(&account.npub);
    let name_span = Span::styled(
        name,
        Style::default()
            .fg(col)
            .bg(row_bg)
            .add_modifier(Modifier::BOLD),
    );
    let pad1_len = content_width.saturating_sub(2 + name_len);
    lines.push(Line::from(vec![
        gutter.clone(),
        active_marker,
        name_span,
        Span::styled(" ".repeat(pad1_len), Style::default().bg(row_bg)),
    ]));

    // Row 2: signer type badge
    let signer = truncate(&account.signer, content_width);
    let signer_len = signer.chars().count();
    let pad2_len = content_width.saturating_sub(signer_len);
    lines.push(Line::from(vec![
        gutter,
        Span::styled(signer, Style::default().fg(DIMMER_TEXT).bg(row_bg)),
        Span::styled(" ".repeat(pad2_len), Style::default().bg(row_bg)),
    ]));
}

fn render_relays(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(LIST_BG))
        .title(Span::styled(
            " Relays ",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    // Prefer relay_edit_rows if available, fall back to live relay diagnostics,
    // then fall back to shared_snapshot_lines for minimal text output.
    let lines = if !state.features.relay_edit_rows.is_empty() {
        // Build a short_url → connection_label lookup from live diagnostics so
        // we show real connection state instead of the role label. short_url
        // strips the wss:// prefix and trailing slash (mirrors kernel logic).
        let conn_map: std::collections::HashMap<String, String> = state
            .relays
            .iter()
            .map(|r| (r.short_url.clone(), r.connection_label.clone()))
            .collect();
        let mut all: Vec<Line<'static>> = Vec::new();
        for row in state.features.relay_edit_rows.iter().take(12) {
            let short = row
                .url
                .strip_prefix("wss://")
                .or_else(|| row.url.strip_prefix("ws://"))
                .unwrap_or(&row.url)
                .trim_end_matches('/');
            let conn_label = conn_map.get(short).cloned().unwrap_or_default();
            let (dot, dot_color) = status_dot(&conn_label);
            let url = truncate(&row.url, pane_width.saturating_sub(4));
            all.push(Line::from(vec![
                Span::styled(format!("{dot} "), Style::default().fg(dot_color)),
                Span::styled(url, Style::default().fg(BODY_TEXT)),
                Span::raw(" "),
                Span::styled(row.role_label.clone(), Style::default().fg(DIM_TEXT)),
            ]));
        }
        all
    } else if !state.relays.is_empty() {
        build_relay_lines(&state.relays, pane_width)
    } else {
        relay_lines(state)
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn build_relay_lines(relays: &[RelayRow], pane_width: usize) -> Vec<Line<'static>> {
    if relays.is_empty() {
        return vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No relay diagnostics yet",
                Style::default().fg(DIM_TEXT),
            )),
        ];
    }
    relays
        .iter()
        .take(10)
        .map(|relay| {
            let (dot, dot_color) = status_dot(&relay.connection_label);
            let count = relay.total_events_display.clone();
            let count_len = count.chars().count();
            let url = truncate(
                &relay.short_url,
                pane_width.saturating_sub(3 + count_len + 1),
            );
            let url_len = url.chars().count();
            let pad_len = pane_width.saturating_sub(2 + url_len + 1 + count_len);
            Line::from(vec![
                Span::styled(format!("{dot} "), Style::default().fg(dot_color)),
                Span::styled(url, Style::default().fg(BODY_TEXT)),
                Span::styled(" ".repeat(pad_len.max(1)), Style::default()),
                Span::styled(count, Style::default().fg(DIM_TEXT)),
            ])
        })
        .collect()
}

fn status_dot(label: &str) -> (char, ratatui::style::Color) {
    let lower = label.to_ascii_lowercase();
    if lower.contains("disconnected") || lower.contains("down") || lower.contains("failed") {
        ('\u{25cb}', RELAY_DOWN)
    } else if lower.contains("connected") || lower == "open" || lower.contains("read") || lower.contains("write") {
        ('\u{25cf}', RELAY_OK)
    } else {
        ('\u{25cc}', RELAY_CONNECTING)
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
