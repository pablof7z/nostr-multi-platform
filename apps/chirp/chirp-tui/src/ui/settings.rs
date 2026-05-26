//! Approach-b Settings tab: 3-pane layout (Accounts | Relays | Outbox).

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AppState;
use crate::feature_snapshot::{
    AccountLine, HistoryRelayLine, OutboxLine, OutboxRelayLine, PublishHistoryLine,
};
use crate::snapshot::RelayRow;
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT, DIMMER_TEXT, LIST_BG, RELAY_CONNECTING,
    RELAY_DOWN, RELAY_OK, REPOST, SELECTED_BG, ZAP, author_color,
};
use crate::ui::shared_snapshot_lines::{action_summary, relay_lines};

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
    render_outbox(frame, cols[2], state);
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

fn render_outbox(frame: &mut Frame, area: Rect, state: &AppState) {
    // When an outbox item is selected, split the pane vertically:
    // top = item list (with cursor), bottom = per-relay detail.
    // When nothing is selected, render the full flat list (legacy behavior).
    let selected = state
        .outbox_selected
        .filter(|i| *i < state.features.outbox.len());

    if let Some(idx) = selected {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);
        render_outbox_list(frame, rows[0], state, Some(idx));
        render_outbox_detail(frame, rows[1], &state.features.outbox[idx]);
    } else {
        render_outbox_list(frame, area, state, None);
    }
}

fn render_outbox_list(frame: &mut Frame, area: Rect, state: &AppState, selected: Option<usize>) {
    let block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            " Outbox ",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Last action summary
    let last_action = action_summary(state);
    lines.push(Line::from(Span::styled(
        last_action,
        Style::default().fg(DIM_TEXT),
    )));
    lines.push(Line::from(""));

    // Outbox summary header
    if !state.features.outbox_summary.title.is_empty() {
        lines.push(Line::from(Span::styled(
            state.features.outbox_summary.title.clone(),
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled("Follows: ", Style::default().fg(DIM_TEXT)),
        Span::styled(
            state.features.follow_count.to_string(),
            Style::default().fg(BODY_TEXT),
        ),
    ]));
    lines.push(Line::from(""));

    if state.features.outbox.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No outbox items",
            Style::default().fg(DIMMER_TEXT),
        )));
    } else {
        for (i, item) in state.features.outbox.iter().take(10).enumerate() {
            append_outbox_line(&mut lines, item, pane_width, selected == Some(i));
        }
    }

    // Settled publish history — read-only list rendered below the active
    // outbox. Skipped entirely when the queue is empty so an empty section
    // header doesn't clutter the pane.
    if !state.features.history.is_empty() {
        append_history_section(&mut lines, &state.features.history, pane_width);
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

/// Render the "Published" history section: a dim divider header followed by
/// one block per settled publish (title + status on the header line, per-relay
/// dot/url/reason rows underneath). History items are NOT selectable — j/k
/// navigation is reserved for the active in-flight outbox above.
fn append_history_section(
    lines: &mut Vec<Line<'static>>,
    history: &[PublishHistoryLine],
    pane_width: usize,
) {
    // Separator line so the eye registers history as a distinct block from
    // the active outbox above. Render the bar/title in dim so the active
    // list stays visually dominant.
    lines.push(Line::from(""));
    let header = build_section_header("Published", pane_width);
    lines.push(Line::from(Span::styled(
        header,
        Style::default().fg(DIM_TEXT),
    )));
    for item in history {
        append_history_item(lines, item, pane_width);
    }
}

/// `"── Published ──────────"` — a unicode box-drawing divider that scales
/// with the pane width. Falls back to a plain ASCII underline at very narrow
/// widths so the header never overflows.
fn build_section_header(title: &str, pane_width: usize) -> String {
    let title_with_pad = format!(" {title} ");
    let title_len = title_with_pad.chars().count();
    if pane_width <= title_len + 4 {
        return title_with_pad.trim().to_string();
    }
    let leading = 2usize;
    let trailing = pane_width.saturating_sub(title_len + leading);
    let mut out = String::with_capacity(pane_width);
    for _ in 0..leading {
        out.push('\u{2500}');
    }
    out.push_str(&title_with_pad);
    for _ in 0..trailing {
        out.push('\u{2500}');
    }
    out
}

fn append_history_item(
    lines: &mut Vec<Line<'static>>,
    item: &PublishHistoryLine,
    pane_width: usize,
) {
    let status_color = history_status_color(&item.status);
    let status = truncate(&item.status, 8);
    let status_len = status.chars().count();
    // Two-space gutter mirrors `append_outbox_line` for visual alignment with
    // the active outbox above (no cursor on history rows — they're read-only).
    let title_max = pane_width.saturating_sub(2 + status_len + 1);
    let title = truncate(&item.title, title_max);
    let title_len = title.chars().count();
    let pad_len = pane_width
        .saturating_sub(2 + title_len + status_len)
        .max(1);
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            title,
            Style::default().fg(BODY_TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(pad_len), Style::default()),
        Span::styled(status, Style::default().fg(status_color)),
    ]));
    for relay in &item.relays {
        append_history_relay_line(lines, relay, pane_width);
    }
    lines.push(Line::from(""));
}

fn append_history_relay_line(
    lines: &mut Vec<Line<'static>>,
    relay: &HistoryRelayLine,
    pane_width: usize,
) {
    let (dot, dot_color) = history_relay_status_dot(&relay.status);
    // Indent under the title row. `pane_width - 4` leaves room for the indent
    // + dot + space at the start of the line.
    let inner_width = pane_width.saturating_sub(4);
    let url = truncate(&relay.relay_url, inner_width);
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{dot} "), Style::default().fg(dot_color)),
        Span::styled(url, Style::default().fg(BODY_TEXT)),
    ]));
    if !relay.relay_reason.is_empty() {
        let reason = truncate(&relay.relay_reason, pane_width.saturating_sub(6));
        lines.push(Line::from(vec![
            Span::raw("      "),
            Span::styled(reason, Style::default().fg(DIM_TEXT)),
        ]));
    }
    if !relay.message.is_empty() {
        let message = truncate(&relay.message, pane_width.saturating_sub(6));
        lines.push(Line::from(vec![
            Span::raw("      "),
            Span::styled(message, Style::default().fg(DIMMER_TEXT)),
        ]));
    }
}

fn history_status_color(status: &str) -> ratatui::style::Color {
    match status {
        "ok" => REPOST,
        "failed" => RELAY_DOWN,
        _ => DIM_TEXT,
    }
}

fn history_relay_status_dot(status: &str) -> (char, ratatui::style::Color) {
    match status {
        "ok" => ('\u{2713}', RELAY_OK),
        "failed" => ('\u{2717}', RELAY_DOWN),
        _ => ('\u{25cc}', RELAY_CONNECTING),
    }
}

fn render_outbox_detail(frame: &mut Frame, area: Rect, item: &OutboxLine) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            " Relays ",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();
    if item.relays.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No relay detail",
            Style::default().fg(DIMMER_TEXT),
        )));
    } else {
        for relay in item.relays.iter() {
            append_outbox_relay_lines(&mut lines, relay, pane_width);
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn append_outbox_line(
    lines: &mut Vec<Line<'static>>,
    item: &OutboxLine,
    pane_width: usize,
    selected: bool,
) {
    let status_color = outbox_status_color(&item.status_label);
    let cursor = if selected { "> " } else { "  " };
    let cursor_color = if selected { ACCENT_CYAN } else { DIM_TEXT };
    let prefix_len = 2; // "> " or "  "
    let handle = truncate(&item.handle, 10);
    let status = truncate(&item.status_label, 8);
    let title_max = pane_width
        .saturating_sub(prefix_len + handle.chars().count() + status.chars().count() + 2);
    let title = truncate(&item.title, title_max);
    lines.push(Line::from(vec![
        Span::styled(cursor.to_string(), Style::default().fg(cursor_color)),
        Span::styled(handle, Style::default().fg(DIM_TEXT)),
        Span::raw(" "),
        Span::styled(status, Style::default().fg(status_color)),
        Span::raw(" "),
        Span::styled(title, Style::default().fg(BODY_TEXT)),
    ]));
}

fn append_outbox_relay_lines(
    lines: &mut Vec<Line<'static>>,
    relay: &OutboxRelayLine,
    pane_width: usize,
) {
    let (dot, dot_color) = relay_status_dot(&relay.status_label);
    let status = truncate(&relay.status_label, 10);
    let status_len = status.chars().count();
    // First line: dot + url ... status
    let url_max = pane_width.saturating_sub(2 + status_len + 1);
    let url = truncate(&relay.relay_url, url_max);
    let url_len = url.chars().count();
    let pad_len = pane_width
        .saturating_sub(2 + url_len + status_len)
        .max(1);
    lines.push(Line::from(vec![
        Span::styled(format!("{dot} "), Style::default().fg(dot_color)),
        Span::styled(url, Style::default().fg(BODY_TEXT)),
        Span::styled(" ".repeat(pad_len), Style::default()),
        Span::styled(status, Style::default().fg(dot_color)),
    ]));
    // Reason line (indented under the URL).
    if !relay.reason.is_empty() {
        let reason = truncate(&relay.reason, pane_width.saturating_sub(2));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(reason, Style::default().fg(DIM_TEXT)),
        ]));
    }
    // Message line (further dimmed).
    if !relay.message.is_empty() {
        let message = truncate(&relay.message, pane_width.saturating_sub(2));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(message, Style::default().fg(DIMMER_TEXT)),
        ]));
    }
    lines.push(Line::from(""));
}

fn relay_status_dot(label: &str) -> (char, ratatui::style::Color) {
    let lower = label.to_ascii_lowercase();
    if lower.contains("ok") || lower.contains("sent") || lower.contains("success") {
        ('\u{25cf}', RELAY_OK)
    } else if lower.contains("fail") || lower.contains("error") || lower.contains("timeout") {
        ('\u{2717}', RELAY_DOWN)
    } else {
        // Pending, Retrying, Sending, etc.
        ('\u{25cc}', RELAY_CONNECTING)
    }
}

fn outbox_status_color(status: &str) -> ratatui::style::Color {
    let lower = status.to_ascii_lowercase();
    if lower.contains("sent") || lower.contains("ok") || lower.contains("success") {
        REPOST
    } else if lower.contains("fail") || lower.contains("error") {
        RELAY_DOWN
    } else {
        ZAP
    }
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
