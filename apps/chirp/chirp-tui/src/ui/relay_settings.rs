//! Relay inventory and detail panes for Settings.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::AppState;
use crate::snapshot::{RelayRow, RelayWireSubRow};
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIMMER_TEXT, DIM_TEXT, LIST_BG, SELECTED_BG, ZAP,
};

mod format;
use format::{
    append_wrapped, compact_count, consumer_count_label, discovery_kinds_label, empty_dash,
    format_bytes, format_ms_ago, label_line, short_relay_url, short_wire_id, status_dot,
    title_case, truncate,
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
        let role = if relay.role.is_empty() {
            "Other".to_string()
        } else {
            title_case(&relay.role)
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
    let (dot, dot_color) = status_dot(&relay.connection);
    let marker = if selected { "\u{2503} " } else { "  " };
    let count = format!("{} ev", compact_count(relay.total_events_rx));
    let count_len = count.chars().count();
    let url_max = pane_width.saturating_sub(4 + count_len);
    let url = truncate(&short_relay_url(&relay.relay_url), url_max);
    let pad_len = pane_width.saturating_sub(4 + url.chars().count() + count_len);
    lines.push(Line::from(vec![
        Span::styled(marker.to_string(), Style::default().fg(ACCENT_CYAN).bg(bg)),
        Span::styled(format!("{dot} "), Style::default().fg(dot_color).bg(bg)),
        Span::styled(url, Style::default().fg(BODY_TEXT).bg(bg)),
        Span::styled(" ".repeat(pad_len.max(1)), Style::default().bg(bg)),
        Span::styled(count, Style::default().fg(DIM_TEXT).bg(bg)),
    ]));

    let cfg = configured.map_or_else(String::new, |role| format!(" · configured {role}"));
    // Append zero-count classification when the relay is connected but has
    // received no session EVENTs (V-51 Phase 3 acceptance criterion 1).
    let zero_annotation =
        zero_count_label(relay).map_or_else(String::new, |label| format!(" · {label}"));
    let status = format!(
        "    {} · {}/{} subs{}{}",
        empty_dash(&title_case(&relay.connection)),
        relay.active_sub_count,
        relay.total_sub_count,
        cfg,
        zero_annotation,
    );
    lines.push(Line::from(Span::styled(
        truncate(&status, pane_width),
        Style::default().fg(DIM_TEXT).bg(bg),
    )));

    // V-51 Phase 3 acceptance criterion 2: for Indexer relays show which
    // discovery kinds they are currently serving (or "none" when no discovery
    // REQ is open).
    if relay.role.eq_ignore_ascii_case("indexer") {
        let disc_line = indexer_discovery_kinds_label(relay);
        let disc_text = format!("    discovery: {disc_line}");
        lines.push(Line::from(Span::styled(
            truncate(&disc_text, pane_width),
            Style::default().fg(DIM_TEXT).bg(bg),
        )));
    }
}

fn relay_detail_lines(state: &AppState, relay: &RelayRow, pane_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        truncate(&relay.relay_url, pane_width),
        Style::default().fg(BODY_TEXT).add_modifier(Modifier::BOLD),
    )));
    lines.push(label_line("role", &empty_dash(&title_case(&relay.role))));
    if let Some(role) = configured_role(state, relay) {
        lines.push(label_line("configured", &role));
    }
    lines.push(label_line(
        "connection",
        &empty_dash(&title_case(&relay.connection)),
    ));
    lines.push(label_line("auth", &empty_dash(&title_case(&relay.auth))));
    lines.push(label_line(
        "events",
        &format!(
            "{} session EVENTs ({})",
            relay.total_events_rx,
            compact_count(relay.total_events_rx)
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
    // V-51 Phase 3: zero-count classification in detail pane when connected
    // relay has received no session EVENTs.
    if let Some(label) = zero_count_label(relay) {
        lines.push(label_line("zero-ev", label));
    }
    // V-51 Phase 3: indexer relay discovery-kind targeting.
    if relay.role.eq_ignore_ascii_case("indexer") {
        lines.push(label_line(
            "discovery",
            &indexer_discovery_kinds_label(relay),
        ));
    }
    lines.push(label_line(
        "traffic",
        &format!(
            "rx {} · tx {} · reconnects {}",
            format_bytes(relay.bytes_rx),
            format_bytes(relay.bytes_tx),
            relay.reconnect_count
        ),
    ));
    lines.push(label_line(
        "last",
        &format!(
            "connected {} · event {}",
            format_ms_ago(relay.last_connected_ms),
            format_ms_ago(relay.last_event_ms)
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
    let events = compact_count(sub.events_rx);
    let header = format!(
        "  {}  {}  {} ev  {}",
        empty_dash(&short_wire_id(&sub.wire_id)),
        empty_dash(&title_case(&sub.state)),
        events,
        empty_dash(&consumer_count_label(sub.consumer_count))
    );
    lines.push(Line::from(Span::styled(
        truncate(&header, pane_width),
        Style::default().fg(BODY_TEXT).add_modifier(Modifier::BOLD),
    )));
    let timing = format!(
        "    opened {} · last {} · eose {}",
        format_ms_ago(sub.opened_ms),
        format_ms_ago(sub.last_event_ms),
        if sub.eose_ms > 0 {
            format_ms_ago(sub.eose_ms)
        } else {
            "not yet".to_string()
        }
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

// ── V-51 Phase 3: zero-count classification ───────────────────────────────

/// Classify why a connected relay has received zero session EVENTs.
///
/// Returns `None` when the relay is not connected or has already received
/// at least one event — the label is only shown when it adds information.
///
/// Classification priority (highest to lowest):
/// 1. `"no REQ"` — no subscription was ever sent to this relay.
/// 2. `"EOSE, no matches"` — relay responded EOSE with zero matching events.
/// 3. `"active REQ, no matches"` — a REQ is open but no events have arrived
///    and EOSE has not been received yet.
/// 4. `"anomaly"` — subscriptions exist but none are active and none
///    observed EOSE; or any state the above three cases do not cover.
pub(crate) fn zero_count_label(relay: &RelayRow) -> Option<&'static str> {
    // Gate: only classify connected relays with zero received events.
    if relay.total_events_rx > 0 {
        return None;
    }
    // Use the same logic as `status_dot`: a relay is "connected" when its
    // label matches the RELAY_OK bucket. Crucially, "Disconnected" must NOT
    // be treated as connected even though it contains "connected" as a
    // substring — we check for "disconnected" first.
    let lower = relay.connection.to_ascii_lowercase();
    if lower.contains("disconnected") || lower.contains("down") || lower.contains("failed") {
        return None;
    }
    let is_connected = lower.contains("connected") || lower == "open" || lower.contains("opening");
    if !is_connected {
        return None;
    }

    if relay.total_sub_count == 0 {
        Some("no REQ")
    } else if relay.eosed_sub_count > 0 {
        // At least one sub received EOSE with zero events — the relay has
        // definitively answered with "no matches". Prefer this label even
        // when other subs are still active, because the EOSE answer is the
        // most informative signal for why the total stays zero.
        Some("EOSE, no matches")
    } else if relay.active_sub_count > 0 {
        Some("active REQ, no matches")
    } else {
        // Subs exist (total_sub_count > 0) but none are active and none
        // observed EOSE — disconnected subs or a state the above branches
        // do not cover.
        Some("anomaly")
    }
}

pub(crate) fn indexer_discovery_kinds_label(relay: &RelayRow) -> String {
    discovery_kinds_label(&relay.discovery_kinds)
}

fn why_text(state: &AppState, relay: &RelayRow) -> String {
    let configured = configured_role(state, relay);
    let mut parts = vec![format!(
        "{} runtime lane",
        empty_dash(&title_case(&relay.role))
    )];
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
        .configured_relays
        .iter()
        .find(|row| {
            short_relay_url(&row.url).eq_ignore_ascii_case(&short_relay_url(&relay.relay_url))
                || row.url.eq_ignore_ascii_case(&relay.relay_url)
        })
        .map(|row| relay_role_display_label(&row.role))
}

fn relay_role_display_label(role: &str) -> String {
    match role {
        "both" => "Both".to_string(),
        "read" => "Read".to_string(),
        "write" => "Write".to_string(),
        "indexer" => "Index".to_string(),
        "both,indexer" => "Both + Index".to_string(),
        "read,indexer" => "Read + Index".to_string(),
        "write,indexer" => "Write + Index".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::RelayRow;

    // ── zero_count_label ──────────────────────────────────────────────────

    fn connected_relay(
        total_sub_count: u64,
        active_sub_count: u64,
        eosed_sub_count: u64,
        total_events_rx: u64,
    ) -> RelayRow {
        RelayRow {
            connection: "connected".to_string(),
            total_sub_count,
            active_sub_count,
            eosed_sub_count,
            total_events_rx,
            ..RelayRow::default()
        }
    }

    #[test]
    fn zero_count_no_req_when_no_subs() {
        let relay = connected_relay(0, 0, 0, 0);
        assert_eq!(zero_count_label(&relay), Some("no REQ"));
    }

    #[test]
    fn zero_count_eose_no_matches_when_eosed_sub_exists() {
        // EOSE label wins even when an active sub is also present (EOSE is
        // the most informative answer — relay has definitively responded).
        let relay = connected_relay(2, 1, 1, 0);
        assert_eq!(zero_count_label(&relay), Some("EOSE, no matches"));
    }

    #[test]
    fn zero_count_active_req_no_matches_when_active_sub_no_eose() {
        let relay = connected_relay(1, 1, 0, 0);
        assert_eq!(zero_count_label(&relay), Some("active REQ, no matches"));
    }

    #[test]
    fn zero_count_anomaly_when_subs_exist_but_none_active_or_eosed() {
        // total_sub_count > 0, active = 0, eosed = 0 => anomaly.
        let relay = connected_relay(1, 0, 0, 0);
        assert_eq!(zero_count_label(&relay), Some("anomaly"));
    }

    #[test]
    fn zero_count_none_when_events_received() {
        let relay = connected_relay(0, 0, 0, 42);
        assert_eq!(zero_count_label(&relay), None);
    }

    #[test]
    fn zero_count_none_when_not_connected() {
        let relay = RelayRow {
            connection: "disconnected".to_string(),
            total_sub_count: 0,
            active_sub_count: 0,
            eosed_sub_count: 0,
            total_events_rx: 0,
            ..RelayRow::default()
        };
        assert_eq!(zero_count_label(&relay), None);
    }

    // ── indexer_discovery_kinds_label ─────────────────────────────────────

    #[test]
    fn indexer_none_when_discovery_kinds_empty() {
        let relay = RelayRow {
            role: "indexer".to_string(),
            ..RelayRow::default()
        };
        assert_eq!(indexer_discovery_kinds_label(&relay), "none");
    }

    #[test]
    fn indexer_shows_projected_discovery_kinds_without_parsing_filters() {
        let relay = RelayRow {
            role: "indexer".to_string(),
            discovery_kinds: vec![0, 3, 10002],
            wire_subs: vec![RelayWireSubRow {
                filter_summary: r#"{"kinds":[1]}"#.to_string(),
                state: "open".to_string(),
                ..RelayWireSubRow::default()
            }],
            ..RelayRow::default()
        };
        let label = indexer_discovery_kinds_label(&relay);
        assert!(
            label.contains("profile (0)"),
            "expected profile in '{label}'"
        );
        assert!(
            label.contains("follows (3)"),
            "expected follows in '{label}'"
        );
        assert!(
            label.contains("relay-list (10002)"),
            "expected relay-list in '{label}'"
        );
    }
}
