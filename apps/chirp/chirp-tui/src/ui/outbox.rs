//! Outbox pane for the Settings tab — active in-flight outbox + settled
//! publish history.
//!
//! Extracted from `ui/settings.rs` to keep that file under the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). The Settings tab's three-pane
//! layout still lives in `settings.rs`; this module owns only the right-hand
//! Outbox pane.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, OutboxSelection};
use crate::feature_snapshot::{HistoryRelayLine, OutboxLine, OutboxRelayLine, PublishHistoryLine};
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIMMER_TEXT, DIM_TEXT, RELAY_CONNECTING, RELAY_DOWN,
    RELAY_OK, REPOST, ZAP,
};
use crate::ui::shared_snapshot_lines::action_summary;

pub(super) fn render_outbox(frame: &mut Frame, area: Rect, state: &AppState) {
    // When an outbox item is selected, split the pane vertically:
    // top = item list (with cursor), bottom = per-relay detail.
    // When nothing is selected, render the full flat list (legacy behavior).
    let selected = state.outbox_selected;

    if let Some(selection) = selected {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);
        render_outbox_list(frame, rows[0], state, Some(selection));
        match selection {
            OutboxSelection::Active(idx) => {
                if let Some(item) = state.features.outbox.get(idx) {
                    render_outbox_detail(frame, rows[1], item);
                }
            }
            OutboxSelection::History(idx) => {
                if let Some(item) = state.features.history.get(idx) {
                    render_history_detail(frame, rows[1], item);
                }
            }
        }
    } else {
        render_outbox_list(frame, area, state, None);
    }
}

fn render_outbox_list(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    selected: Option<OutboxSelection>,
) {
    let block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            " Outbox ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
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
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
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
            append_outbox_line(
                &mut lines,
                item,
                pane_width,
                selected == Some(OutboxSelection::Active(i)),
            );
        }
    }

    // Settled publish history — read-only list rendered below the active
    // outbox. Skipped entirely when the queue is empty so an empty section
    // header doesn't clutter the pane.
    if !state.features.history.is_empty() {
        append_history_section(&mut lines, &state.features.history, pane_width, selected);
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

/// Render the "Published" history section: a dim divider header followed by
/// one block per settled publish (title + status on the header line, per-relay
/// dot/url/reason rows underneath).
fn append_history_section(
    lines: &mut Vec<Line<'static>>,
    history: &[PublishHistoryLine],
    pane_width: usize,
    selected: Option<OutboxSelection>,
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
    for (i, item) in history.iter().enumerate() {
        append_history_item(
            lines,
            item,
            pane_width,
            selected == Some(OutboxSelection::History(i)),
        );
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
    selected: bool,
) {
    let status_color = history_status_color(&item.status);
    let cursor = if selected { "> " } else { "  " };
    let cursor_color = if selected { ACCENT_CYAN } else { DIM_TEXT };
    let status = truncate(&item.status, 8);
    let status_len = status.chars().count();
    let title_max = pane_width.saturating_sub(2 + status_len + 1);
    let title = truncate(&item.title, title_max);
    let title_len = title.chars().count();
    let pad_len = pane_width.saturating_sub(2 + title_len + status_len).max(1);
    lines.push(Line::from(vec![
        Span::styled(cursor.to_string(), Style::default().fg(cursor_color)),
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
            " Active Publish ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
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
        lines.push(Line::from(vec![
            Span::styled("  handle ", Style::default().fg(DIM_TEXT)),
            Span::styled(item.handle.clone(), Style::default().fg(BODY_TEXT)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  action ", Style::default().fg(DIM_TEXT)),
            Span::styled(
                "r retry  d cancel  Esc close",
                Style::default().fg(BODY_TEXT),
            ),
        ]));
        lines.push(Line::from(""));
        for relay in item.relays.iter() {
            append_outbox_relay_lines(&mut lines, relay, pane_width);
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn render_history_detail(frame: &mut Frame, area: Rect, item: &PublishHistoryLine) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            " Published Detail ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("  status ", Style::default().fg(DIM_TEXT)),
        Span::styled(
            item.status.clone(),
            Style::default().fg(history_status_color(&item.status)),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  event  ", Style::default().fg(DIM_TEXT)),
        Span::styled(
            truncate(&item.event_id, pane_width.saturating_sub(9)),
            Style::default().fg(BODY_TEXT),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  kind   ", Style::default().fg(DIM_TEXT)),
        Span::styled(
            format!("{} ({})", item.title, item.kind),
            Style::default().fg(BODY_TEXT),
        ),
    ]));
    let actions = if item.can_retry {
        "r retry  d clear  Esc close"
    } else {
        "d clear  Esc close"
    };
    lines.push(Line::from(vec![
        Span::styled("  action ", Style::default().fg(DIM_TEXT)),
        Span::styled(actions, Style::default().fg(BODY_TEXT)),
    ]));
    lines.push(Line::from(""));
    if item.relays.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No relay verdicts were recorded",
            Style::default().fg(DIMMER_TEXT),
        )));
    } else {
        for relay in &item.relays {
            append_history_relay_line(&mut lines, relay, pane_width);
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
    let title_max =
        pane_width.saturating_sub(prefix_len + handle.chars().count() + status.chars().count() + 2);
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
    let pad_len = pane_width.saturating_sub(2 + url_len + status_len).max(1);
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
