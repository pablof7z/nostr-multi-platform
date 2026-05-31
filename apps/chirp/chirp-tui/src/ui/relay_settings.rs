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

// ── Discovery-kind classification constants ────────────────────────────────

/// Discovery kinds per the V-51 acceptance criterion: profile (0), follow-list
/// (3), relay-list (10002), and the generic replaceable-list range (10000–19999).
/// These are the kinds that should be fetched from Indexer relays.
const DISCOVERY_KINDS: &[u64] = &[0, 3, 10002];
const DISCOVERY_LIST_RANGE: std::ops::RangeInclusive<u64> = 10000..=19999;

/// Human-readable name for a discovery kind, used in the indexer summary line.
fn discovery_kind_label(kind: u64) -> &'static str {
    match kind {
        0 => "profile",
        3 => "follows",
        10002 => "relay-list",
        _ => "list",
    }
}

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
    // Append zero-count classification when the relay is connected but has
    // received no session EVENTs (V-51 Phase 3 acceptance criterion 1).
    let zero_annotation = zero_count_label(relay)
        .map_or_else(String::new, |label| format!(" · {label}"));
    let status = format!(
        "    {} · {}/{} subs{}{}",
        empty_dash(&relay.connection_label),
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
    if relay.role_label.eq_ignore_ascii_case("indexer") {
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
    // V-51 Phase 3: zero-count classification in detail pane when connected
    // relay has received no session EVENTs.
    if let Some(label) = zero_count_label(relay) {
        lines.push(label_line("zero-ev", label));
    }
    // V-51 Phase 3: indexer relay discovery-kind targeting.
    if relay.role_label.eq_ignore_ascii_case("indexer") {
        lines.push(label_line("discovery", &indexer_discovery_kinds_label(relay)));
    }
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
    let lower = relay.connection_label.to_ascii_lowercase();
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

// ── V-51 Phase 3: indexer discovery-kind targeting ────────────────────────

/// Check whether a kind is a discovery kind per the V-51 criterion.
fn is_discovery_kind(kind: u64) -> bool {
    DISCOVERY_KINDS.contains(&kind) || DISCOVERY_LIST_RANGE.contains(&kind)
}

/// Parse the kinds from a `filter_summary` JSON string.
///
/// The field is built by `wire::filter_json_for` using `serde_json::to_string`
/// on a `nostr::Filter` struct, so it is always valid compact JSON. We parse it
/// to extract the `kinds` array without pulling in a full JSON dependency in
/// this render module — we use `serde_json` which is already a workspace dep.
fn kinds_from_filter_summary(filter_summary: &str) -> Vec<u64> {
    serde_json::from_str::<serde_json::Value>(filter_summary)
        .ok()
        .and_then(|v| v.get("kinds").cloned())
        .and_then(|k| k.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|k| k.as_u64())
        .collect()
}

/// Build a summary line for which discovery kinds an Indexer relay is
/// currently serving (has open/active wire subscriptions for).
///
/// Example output: `"profile (0), follows (3), relay-list (10002)"`
/// When no discovery REQ is open: `"none"`
pub(crate) fn indexer_discovery_kinds_label(relay: &RelayRow) -> String {
    // Collect the union of all discovery kinds from active wire subscriptions.
    // We include any subscription that is not closed (state not "closed" or
    // "closing") so paused/opening subs are also reflected.
    let mut found: Vec<u64> = relay
        .wire_subs
        .iter()
        .filter(|sub| {
            let state = sub.state_label.to_ascii_lowercase();
            !state.contains("closed") && !state.contains("closing")
        })
        .flat_map(|sub| kinds_from_filter_summary(&sub.filter_summary))
        .filter(|k| is_discovery_kind(*k))
        .collect();

    // Deduplicate and sort for deterministic output.
    found.sort_unstable();
    found.dedup();

    if found.is_empty() {
        "none".to_string()
    } else {
        found
            .into_iter()
            .map(|k| format!("{} ({})", discovery_kind_label(k), k))
            .collect::<Vec<_>>()
            .join(", ")
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
            connection_label: "Connected".to_string(),
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
            connection_label: "Disconnected".to_string(),
            total_sub_count: 0,
            active_sub_count: 0,
            eosed_sub_count: 0,
            total_events_rx: 0,
            ..RelayRow::default()
        };
        assert_eq!(zero_count_label(&relay), None);
    }

    // ── indexer_discovery_kinds_label ─────────────────────────────────────

    fn make_wire_sub(filter_summary: &str, state_label: &str) -> RelayWireSubRow {
        RelayWireSubRow {
            filter_summary: filter_summary.to_string(),
            state_label: state_label.to_string(),
            ..RelayWireSubRow::default()
        }
    }

    #[test]
    fn indexer_none_when_no_wire_subs() {
        let relay = RelayRow {
            role_label: "Indexer".to_string(),
            ..RelayRow::default()
        };
        assert_eq!(indexer_discovery_kinds_label(&relay), "none");
    }

    #[test]
    fn indexer_shows_discovery_kinds_from_open_subs() {
        let relay = RelayRow {
            role_label: "Indexer".to_string(),
            wire_subs: vec![
                make_wire_sub(r#"{"kinds":[0,3],"authors":["ab"]}"#, "Open"),
                make_wire_sub(r#"{"kinds":[10002],"authors":["cd"]}"#, "Open"),
            ],
            ..RelayRow::default()
        };
        let label = indexer_discovery_kinds_label(&relay);
        // Should list all three discovery kinds found across the active subs.
        assert!(label.contains("profile (0)"), "expected profile in '{label}'");
        assert!(label.contains("follows (3)"), "expected follows in '{label}'");
        assert!(
            label.contains("relay-list (10002)"),
            "expected relay-list in '{label}'"
        );
    }

    #[test]
    fn indexer_excludes_closed_subs() {
        let relay = RelayRow {
            role_label: "Indexer".to_string(),
            wire_subs: vec![
                make_wire_sub(r#"{"kinds":[0],"authors":["ab"]}"#, "Closed"),
                make_wire_sub(r#"{"kinds":[3],"authors":["cd"]}"#, "Open"),
            ],
            ..RelayRow::default()
        };
        let label = indexer_discovery_kinds_label(&relay);
        assert!(!label.contains("profile"), "closed sub must be excluded");
        assert!(label.contains("follows (3)"), "open sub must be included");
    }

    #[test]
    fn indexer_non_discovery_kinds_not_shown() {
        let relay = RelayRow {
            role_label: "Indexer".to_string(),
            wire_subs: vec![make_wire_sub(r#"{"kinds":[1,6]}"#, "Open")],
            ..RelayRow::default()
        };
        let label = indexer_discovery_kinds_label(&relay);
        // Kinds 1 (text-note) and 6 (repost) are not discovery kinds.
        assert_eq!(label, "none");
    }

    #[test]
    fn indexer_list_range_kinds_included() {
        // Kinds 10003–19999 (generic replaceable lists) should be included.
        let relay = RelayRow {
            role_label: "Indexer".to_string(),
            wire_subs: vec![make_wire_sub(r#"{"kinds":[10003]}"#, "Open")],
            ..RelayRow::default()
        };
        let label = indexer_discovery_kinds_label(&relay);
        assert!(
            label.contains("10003"),
            "list-range kind 10003 must appear in '{label}'"
        );
    }

    // ── is_discovery_kind ─────────────────────────────────────────────────

    #[test]
    fn discovery_kind_boundaries() {
        assert!(is_discovery_kind(0));
        assert!(is_discovery_kind(3));
        assert!(is_discovery_kind(10002));
        assert!(is_discovery_kind(10000));
        assert!(is_discovery_kind(19999));
        assert!(!is_discovery_kind(1));
        assert!(!is_discovery_kind(9999));
        assert!(!is_discovery_kind(20000));
    }
}
