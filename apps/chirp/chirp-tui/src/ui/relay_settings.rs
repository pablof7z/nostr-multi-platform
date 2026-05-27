//! Relay inventory and detail panes for Settings.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::AppState;
use crate::snapshot::{RelayRow, RelayWireSubRow};
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIMMER_TEXT, DIM_TEXT, LIST_BG, RELAY_CONNECTING,
    RELAY_DOWN, RELAY_OK, SELECTED_BG, ZAP,
};

pub(super) fn render_relay_list(frame: &mut Frame, area: Rect, state: &AppState, active: bool) {
    let title = if state.relays.is_empty() {
        " All Relays ".to_string()
    } else {
        format!(" All Relays {} ", state.relays.len())
    };
    let border = if active { ACCENT_CYAN } else { DIMMER_TEXT };
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(LIST_BG))
        .title(Span::styled(
            title,
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ));
    let pane_width = block.inner(area).width as usize;
    let lines = relay_list_lines(state, pane_width);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

pub(super) fn render_relay_detail(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            " Relay Detail ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ));
    let pane_width = block.inner(area).width as usize;
    let lines = state
        .relays
        .get(state.settings_relay_selected)
        .map(|relay| relay_detail_lines(state, relay, pane_width))
        .unwrap_or_else(|| {
            vec![Line::from(Span::styled(
                "  No relay diagnostics yet",
                Style::default().fg(DIM_TEXT),
            ))]
        });
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn relay_list_lines(state: &AppState, pane_width: usize) -> Vec<Line<'static>> {
    if state.relays.is_empty() {
        return vec![Line::from(Span::styled(
            "  No relay diagnostics yet",
            Style::default().fg(DIM_TEXT),
        ))];
    }

    let mut lines = Vec::new();
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (idx, relay) in state.relays.iter().enumerate() {
        let role = if relay.role_label.is_empty() {
            "Other".to_string()
        } else {
            relay.role_label.clone()
        };
        if let Some((_, rows)) = groups.iter_mut().find(|(label, _)| label == &role) {
            rows.push(idx);
        } else {
            groups.push((role, vec![idx]));
        }
    }

    for (role, indices) in groups {
        lines.push(Line::from(Span::styled(
            format!(" {role}"),
            Style::default().fg(ZAP).add_modifier(Modifier::BOLD),
        )));
        for idx in indices {
            let relay = &state.relays[idx];
            append_relay_row(
                &mut lines,
                relay,
                state.settings_relay_selected == idx,
                pane_width,
                configured_role(state, relay),
            );
        }
    }
    lines
}

fn append_relay_row(
    lines: &mut Vec<Line<'static>>,
    relay: &RelayRow,
    selected: bool,
    pane_width: usize,
    configured: Option<String>,
) {
    let bg = if selected { SELECTED_BG } else { LIST_BG };
    let (dot, dot_color) = status_dot(&relay.connection_label);
    let marker = if selected { "\u{2503} " } else { "  " };
    let count = format!("{} ev", relay.total_events_display);
    let count_len = count.chars().count();
    let url_max = pane_width.saturating_sub(4 + count_len);
    let url = truncate(&relay.short_url, url_max);
    let pad_len = pane_width.saturating_sub(4 + url.chars().count() + count_len);
    lines.push(Line::from(vec![
        Span::styled(marker.to_string(), Style::default().fg(ACCENT_CYAN).bg(bg)),
        Span::styled(format!("{dot} "), Style::default().fg(dot_color).bg(bg)),
        Span::styled(url, Style::default().fg(BODY_TEXT).bg(bg)),
        Span::styled(" ".repeat(pad_len.max(1)), Style::default().bg(bg)),
        Span::styled(count, Style::default().fg(DIM_TEXT).bg(bg)),
    ]));

    let cfg = configured.map_or_else(String::new, |role| format!(" · configured {role}"));
    let status = format!(
        "    {} · {}/{} subs{}",
        empty_dash(&relay.connection_label),
        relay.active_sub_count,
        relay.total_sub_count,
        cfg
    );
    lines.push(Line::from(Span::styled(
        truncate(&status, pane_width),
        Style::default().fg(DIM_TEXT).bg(bg),
    )));
}

fn relay_detail_lines(state: &AppState, relay: &RelayRow, pane_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        truncate(&relay.relay_url, pane_width),
        Style::default().fg(BODY_TEXT).add_modifier(Modifier::BOLD),
    )));
    lines.push(label_line("role", &empty_dash(&relay.role_label)));
    if let Some(role) = configured_role(state, relay) {
        lines.push(label_line("configured", &role));
    }
    lines.push(label_line(
        "connection",
        &empty_dash(&relay.connection_label),
    ));
    lines.push(label_line("auth", &empty_dash(&relay.auth_label)));
    lines.push(label_line(
        "events",
        &format!(
            "{} session EVENTs ({})",
            relay.total_events_rx, relay.total_events_display
        ),
    ));
    lines.push(label_line(
        "subs",
        &format!(
            "{} active / {} total / {} EOSE",
            relay.active_sub_count, relay.total_sub_count, relay.eosed_sub_count
        ),
    ));
    lines.push(label_line("why", &why_text(state, relay)));
    lines.push(label_line(
        "traffic",
        &format!(
            "rx {} · tx {} · reconnects {}",
            relay.bytes_rx_display.as_deref().unwrap_or("0 B"),
            relay.bytes_tx_display.as_deref().unwrap_or("0 B"),
            relay.reconnect_count
        ),
    ));
    lines.push(label_line(
        "last",
        &format!(
            "connected {} · event {}",
            relay.last_connected_display.as_deref().unwrap_or("never"),
            relay.last_event_display.as_deref().unwrap_or("never")
        ),
    ));
    if let Some(notice) = &relay.last_notice {
        append_wrapped(&mut lines, "notice", notice, pane_width);
    }
    if let Some(error) = &relay.last_error {
        append_wrapped(&mut lines, "error", error, pane_width);
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Subscriptions",
        Style::default()
            .fg(ACCENT_CYAN)
            .add_modifier(Modifier::BOLD),
    )));
    if relay.wire_subs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No wire subscriptions on this relay",
            Style::default().fg(DIM_TEXT),
        )));
    } else {
        for sub in &relay.wire_subs {
            append_wire_sub(&mut lines, sub, pane_width);
        }
    }
    lines
}

fn append_wire_sub(lines: &mut Vec<Line<'static>>, sub: &RelayWireSubRow, pane_width: usize) {
    let events = sub.events_rx_display.as_deref().unwrap_or("0");
    let header = format!(
        "  {}  {}  {} ev  {}",
        empty_dash(&sub.short_wire_id),
        empty_dash(&sub.state_label),
        events,
        empty_dash(&sub.consumer_count_label)
    );
    lines.push(Line::from(Span::styled(
        truncate(&header, pane_width),
        Style::default().fg(BODY_TEXT).add_modifier(Modifier::BOLD),
    )));
    let timing = format!(
        "    opened {} · last {} · eose {}",
        empty_dash(&sub.opened_display),
        sub.last_event_display.as_deref().unwrap_or("never"),
        sub.eose_display.as_deref().unwrap_or("not yet")
    );
    lines.push(Line::from(Span::styled(
        truncate(&timing, pane_width),
        Style::default().fg(DIM_TEXT),
    )));
    append_wrapped(lines, "raw", &sub.filter_summary, pane_width);
    if let Some(reason) = &sub.close_reason {
        append_wrapped(lines, "close", reason, pane_width);
    }
}

fn why_text(state: &AppState, relay: &RelayRow) -> String {
    let configured = configured_role(state, relay);
    let mut parts = vec![format!("{} runtime lane", empty_dash(&relay.role_label))];
    if let Some(role) = configured {
        parts.push(format!("configured app relay ({role})"));
    }
    if relay.active_sub_count > 0 {
        parts.push(format!("{} active REQ(s)", relay.active_sub_count));
    } else if relay.total_sub_count > 0 {
        parts.push("wire subscriptions are not active".to_string());
    } else {
        parts.push("no active wire subscriptions".to_string());
    }
    parts.join(" · ")
}

fn configured_role(state: &AppState, relay: &RelayRow) -> Option<String> {
    state
        .features
        .relay_edit_rows
        .iter()
        .find(|row| {
            short_relay_url(&row.url).eq_ignore_ascii_case(&relay.short_url)
                || row.url.eq_ignore_ascii_case(&relay.relay_url)
        })
        .map(|row| row.role_label.clone())
}

fn label_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(DIM_TEXT)),
        Span::styled(value.to_string(), Style::default().fg(BODY_TEXT)),
    ])
}

fn append_wrapped(lines: &mut Vec<Line<'static>>, label: &str, value: &str, pane_width: usize) {
    let prefix = format!("{label}: ");
    let available = pane_width.saturating_sub(prefix.chars().count()).max(8);
    let mut chunks = wrap_chunks(value, available);
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    for (idx, chunk) in chunks.into_iter().enumerate() {
        if idx == 0 {
            lines.push(Line::from(vec![
                Span::styled(prefix.clone(), Style::default().fg(DIM_TEXT)),
                Span::styled(chunk, Style::default().fg(BODY_TEXT)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(" ".repeat(prefix.chars().count()), Style::default()),
                Span::styled(chunk, Style::default().fg(BODY_TEXT)),
            ]));
        }
    }
}

fn wrap_chunks(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        if current.chars().count() >= width {
            chunks.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn status_dot(connection_label: &str) -> (char, ratatui::style::Color) {
    let lower = connection_label.to_ascii_lowercase();
    if lower.contains("disconnected") || lower.contains("down") || lower.contains("failed") {
        ('\u{25cb}', RELAY_DOWN)
    } else if lower.contains("connected") || lower == "open" {
        ('\u{25cf}', RELAY_OK)
    } else {
        ('\u{25cc}', RELAY_CONNECTING)
    }
}

fn short_relay_url(url: &str) -> String {
    url.strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_string()
}

fn empty_dash(value: &str) -> String {
    if value.is_empty() {
        "-".to_string()
    } else {
        value.to_string()
    }
}

fn truncate(value: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = value.chars().count();
    if count <= max {
        value.to_string()
    } else if max <= 3 {
        value.chars().take(max).collect()
    } else {
        let mut out: String = value.chars().take(max.saturating_sub(3)).collect();
        out.push_str("...");
        out
    }
}
