//! Approach-b Settings tab: 2-pane master-detail layout.
//!
//! Left pane (35%): section list with 6 sections.
//! Right pane (65%): section-specific content.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AppState;
use crate::feature_snapshot::AccountLine;
use crate::snapshot::RelayRow;
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT, DIMMER_TEXT, LIST_BG, RELAY_CONNECTING,
    RELAY_DOWN, RELAY_OK, SELECTED_BG,
};

const SECTION_NAMES: [&str; 6] = [
    "Account",
    "Relays",
    "Outbox",
    "Keys",
    "Appearance",
    "About",
];

/// Fallback cursor — the wiring agent will replace this with `state.settings_cursor`.
fn settings_cursor(_state: &AppState) -> usize {
    // use state.settings_cursor if it exists, else 0
    0 // fallback — wiring agent will replace
}

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    render_section_list(f, cols[0], state);
    render_section_content(f, cols[1], state);
}

// ---------------------------------------------------------------------------
// Left pane — section list
// ---------------------------------------------------------------------------

fn render_section_list(f: &mut Frame, area: Rect, state: &AppState) {
    let cursor = settings_cursor(state);

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(LIST_BG));

    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(""));

    for (i, name) in SECTION_NAMES.iter().enumerate() {
        let selected = i == cursor;
        let row_bg = if selected { SELECTED_BG } else { LIST_BG };

        if selected {
            let gutter_span =
                Span::styled("\u{2503} ", Style::default().fg(ACCENT_CYAN).bg(row_bg));
            let label_max = pane_width.saturating_sub(2);
            let label = truncate(name, label_max);
            let label_len = label.chars().count();
            let pad_len = pane_width.saturating_sub(2 + label_len);
            lines.push(Line::from(vec![
                gutter_span,
                Span::styled(
                    label,
                    Style::default()
                        .fg(BODY_TEXT)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ".repeat(pad_len), Style::default().bg(row_bg)),
            ]));
        } else {
            let label = truncate(name, pane_width.saturating_sub(2));
            let label_len = label.chars().count();
            let pad_len = pane_width.saturating_sub(2 + label_len);
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default().bg(row_bg)),
                Span::styled(label, Style::default().fg(DIM_TEXT).bg(row_bg)),
                Span::styled(" ".repeat(pad_len), Style::default().bg(row_bg)),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(LIST_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Right pane — section content dispatcher
// ---------------------------------------------------------------------------

fn render_section_content(f: &mut Frame, area: Rect, state: &AppState) {
    let cursor = settings_cursor(state);
    match cursor {
        0 => render_section_account(f, area, state),
        1 => render_section_relays(f, area, state),
        2 => render_section_outbox(f, area, state),
        3 => render_section_keys(f, area),
        4 => render_section_appearance(f, area),
        5 => render_section_about(f, area, state),
        _ => render_section_account(f, area, state),
    }
}

// ---------------------------------------------------------------------------
// Section 0: Account
// ---------------------------------------------------------------------------

fn render_section_account(f: &mut Frame, area: Rect, state: &AppState) {
    let block = section_block("Account");
    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Active account summary
    let active_display = if state.features.active_account.is_empty() {
        "none".to_string()
    } else {
        state.features.active_account.clone()
    };
    lines.push(Line::from(vec![
        Span::styled("  Active:  ", Style::default().fg(DIM_TEXT)),
        Span::styled(active_display, Style::default().fg(BODY_TEXT)),
    ]));
    lines.push(Line::from(""));

    if state.features.accounts.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No accounts configured.",
            Style::default().fg(DIMMER_TEXT),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  All accounts:",
            Style::default().fg(DIM_TEXT),
        )));
        lines.push(Line::from(""));
        for account in state.features.accounts.iter() {
            lines.push(account_line(account, pane_width));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("[n]", Style::default().fg(ACCENT_CYAN)),
        Span::raw(" Add account  "),
        Span::styled("[d]", Style::default().fg(ACCENT_CYAN)),
        Span::raw(" Remove selected  "),
        Span::styled("[a]", Style::default().fg(ACCENT_CYAN)),
        Span::raw(" Switch"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

fn account_line(account: &AccountLine, pane_width: usize) -> Line<'static> {
    let (dot, dot_color) = if account.active {
        ('\u{25cf}', RELAY_OK) // ●
    } else {
        ('\u{25cb}', DIM_TEXT) // ○
    };

    let display = truncate(&account.display, 16);
    let npub = truncate(&account.npub, 14);
    let signer = truncate(&account.signer, 12);
    let used = 4 + display.chars().count() + 3 + npub.chars().count() + 3 + signer.chars().count();
    let pad_len = pane_width.saturating_sub(used);

    Line::from(vec![
        Span::raw("   "),
        Span::styled(dot.to_string(), Style::default().fg(dot_color)),
        Span::raw(" "),
        Span::styled(display, Style::default().fg(BODY_TEXT)),
        Span::raw("   "),
        Span::styled(npub, Style::default().fg(DIM_TEXT)),
        Span::raw("   "),
        Span::styled(signer, Style::default().fg(DIMMER_TEXT)),
        Span::raw(" ".repeat(pad_len)),
    ])
}

// ---------------------------------------------------------------------------
// Section 1: Relays
// ---------------------------------------------------------------------------

fn render_section_relays(f: &mut Frame, area: Rect, state: &AppState) {
    let block = section_block("Relays");
    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Legend
    lines.push(Line::from(vec![
        Span::styled(" \u{25cf} ", Style::default().fg(RELAY_OK)),
        Span::styled("online  ", Style::default().fg(DIM_TEXT)),
        Span::styled("\u{25d0} ", Style::default().fg(RELAY_CONNECTING)),
        Span::styled("degraded  ", Style::default().fg(DIM_TEXT)),
        Span::styled("\u{25cb} ", Style::default().fg(RELAY_DOWN)),
        Span::styled("offline", Style::default().fg(DIM_TEXT)),
    ]));
    lines.push(Line::from(""));

    if state.relays.is_empty() && state.features.relay_edit_rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No relays configured.",
            Style::default().fg(DIMMER_TEXT),
        )));
    } else {
        let relay_rows = build_merged_relay_lines(&state.relays, state, pane_width);
        lines.extend(relay_rows);
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("[n]", Style::default().fg(ACCENT_CYAN)),
        Span::raw(" Add relay   "),
        Span::styled("[d]", Style::default().fg(ACCENT_CYAN)),
        Span::raw(" Remove   "),
        Span::styled("[Space]", Style::default().fg(ACCENT_CYAN)),
        Span::raw(" Toggle role"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

fn build_merged_relay_lines(
    live: &[RelayRow],
    state: &AppState,
    pane_width: usize,
) -> Vec<Line<'static>> {
    // Prefer live rows if present; augment with role from relay_edit_rows.
    if !live.is_empty() {
        live.iter()
            .take(12)
            .map(|row| {
                let (dot, dot_color) = status_dot_conn(&row.connection_label);
                // Find matching role from relay_edit_rows (suffix match).
                let role = state
                    .features
                    .relay_edit_rows
                    .iter()
                    .find(|r| r.url.ends_with(&row.short_url) || row.short_url.contains(&r.url))
                    .map(|r| r.role_label.clone())
                    .or_else(|| {
                        if row.role_label.is_empty() {
                            None
                        } else {
                            Some(row.role_label.clone())
                        }
                    })
                    .unwrap_or_else(|| row.role_label.clone());

                let count = row.total_events_display.clone();
                let url = truncate(&row.short_url, pane_width.saturating_sub(4 + count.chars().count() + role.chars().count() + 4));
                Line::from(vec![
                    Span::raw("   "),
                    Span::styled(dot.to_string(), Style::default().fg(dot_color)),
                    Span::raw(" "),
                    Span::styled(url, Style::default().fg(BODY_TEXT)),
                    Span::raw("  "),
                    Span::styled(count, Style::default().fg(DIM_TEXT)),
                    Span::raw("  "),
                    Span::styled(role, Style::default().fg(DIMMER_TEXT)),
                ])
            })
            .collect()
    } else {
        // Fall back to relay_edit_rows only
        state
            .features
            .relay_edit_rows
            .iter()
            .take(12)
            .map(|row| {
                let url = truncate(&row.url, pane_width.saturating_sub(4 + row.role_label.chars().count() + 2));
                Line::from(vec![
                    Span::raw("   "),
                    Span::styled("\u{25cc}", Style::default().fg(RELAY_CONNECTING)),
                    Span::raw(" "),
                    Span::styled(url, Style::default().fg(BODY_TEXT)),
                    Span::raw("  "),
                    Span::styled(row.role_label.clone(), Style::default().fg(DIMMER_TEXT)),
                ])
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Section 2: Outbox
// ---------------------------------------------------------------------------

fn render_section_outbox(f: &mut Frame, area: Rect, state: &AppState) {
    let block = section_block("Outbox");
    let inner = block.inner(area);
    let pane_width = inner.width as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Summary counts
    let pending = state
        .features
        .outbox
        .iter()
        .filter(|o| {
            let s = o.status_label.to_ascii_lowercase();
            !s.contains("done") && !s.contains("fail")
        })
        .count();
    let failed = state
        .features
        .outbox
        .iter()
        .filter(|o| o.status_label.to_ascii_lowercase().contains("fail"))
        .count();

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{pending} pending"), Style::default().fg(BODY_TEXT)),
        Span::raw("  \u{00b7}  "),
        Span::styled(format!("{failed} failed"), Style::default().fg(DIM_TEXT)),
    ]));
    lines.push(Line::from(""));

    if state.features.outbox.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No outbox items.",
            Style::default().fg(DIMMER_TEXT),
        )));
    } else {
        for item in state.features.outbox.iter().take(12) {
            let status_lower = item.status_label.to_ascii_lowercase();
            let icon = if status_lower.contains("fail") {
                "\u{2717}" // ✗
            } else {
                "\u{21bb}" // ⟳
            };
            let status_color = if status_lower.contains("fail") || status_lower.contains("error") {
                RELAY_DOWN
            } else if status_lower.contains("done") || status_lower.contains("ok") {
                RELAY_OK
            } else {
                RELAY_CONNECTING
            };

            let title = truncate(&item.title, pane_width.saturating_sub(30));
            let status = truncate(&item.status_label, 12);
            let hint: Vec<Span<'static>> = if item.can_retry {
                vec![
                    Span::raw("  "),
                    Span::styled("[r]", Style::default().fg(ACCENT_CYAN)),
                    Span::raw(" retry"),
                ]
            } else {
                vec![]
            };

            let mut row_spans: Vec<Span<'static>> = vec![
                Span::raw("   "),
                Span::styled(icon.to_string(), Style::default().fg(status_color)),
                Span::raw(" "),
                Span::styled(title, Style::default().fg(BODY_TEXT)),
                Span::raw("  "),
                Span::styled(status, Style::default().fg(status_color)),
            ];
            row_spans.extend(hint);

            lines.push(Line::from(row_spans));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Section 3: Keys (keymap reference)
// ---------------------------------------------------------------------------

fn render_section_keys(f: &mut Frame, area: Rect) {
    let block = section_block("Keys");

    // Two-column layout inside the right pane
    static KEYBINDINGS: &[(&str, &str)] = &[
        ("j / k", "move selection"),
        ("PgUp / PgDn", "page feed"),
        ("Home / End", "top / bottom"),
        ("1 2 3", "feed / detail / profile"),
        ("h c g w s", "switch tabs"),
        ("Tab", "cycle tabs"),
        (":", "command mode"),
        ("Enter", "open thread"),
        ("p", "open author"),
        ("i", "compose note"),
        ("r", "reply"),
        ("+", "react"),
        ("f / F", "follow / unfollow"),
        ("Ctrl+Enter", "publish compose"),
        ("Esc", "cancel / close"),
    ];

    let half = (KEYBINDINGS.len() + 1) / 2;
    let inner = block.inner(area);
    let pane_width = inner.width as usize;
    let col_width = pane_width / 2;

    let mut lines: Vec<Line<'static>> = vec![Line::from("")];
    for i in 0..half {
        let left = KEYBINDINGS[i];
        let right = KEYBINDINGS.get(i + half);

        let left_key = format!("{:<14}", left.0);
        let left_desc = truncate(left.1, col_width.saturating_sub(16));

        let right_spans: Vec<Span<'static>> = if let Some(r) = right {
            let right_key = format!("{:<14}", r.0);
            let right_desc = truncate(r.1, col_width.saturating_sub(16));
            vec![
                Span::styled(right_key, Style::default().fg(ACCENT_CYAN)),
                Span::styled(right_desc, Style::default().fg(BODY_TEXT)),
            ]
        } else {
            vec![]
        };

        let mut spans: Vec<Span<'static>> = vec![
            Span::raw("  "),
            Span::styled(left_key, Style::default().fg(ACCENT_CYAN)),
            Span::styled(left_desc, Style::default().fg(BODY_TEXT)),
            Span::raw("  "),
        ];
        spans.extend(right_spans);
        lines.push(Line::from(spans));
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Section 4: Appearance (static)
// ---------------------------------------------------------------------------

fn render_section_appearance(f: &mut Frame, area: Rect) {
    let block = section_block("Appearance");

    let lines: Vec<Line<'static>> = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Theme:     ", Style::default().fg(DIM_TEXT)),
            Span::styled("Dark (default)", Style::default().fg(BODY_TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Density:   ", Style::default().fg(DIM_TEXT)),
            Span::styled("Comfortable", Style::default().fg(BODY_TEXT)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  (appearance settings coming soon)",
            Style::default().fg(DIMMER_TEXT),
        )),
    ];

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Section 5: About
// ---------------------------------------------------------------------------

fn render_section_about(f: &mut Frame, area: Rect, state: &AppState) {
    let block = section_block("About");

    let nmp_status = if state.relays.is_empty() {
        "starting"
    } else {
        "connected"
    };

    let lines: Vec<Line<'static>> = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  chirp-tui  ", Style::default().fg(BODY_TEXT).add_modifier(Modifier::BOLD)),
            Span::styled("v0.1.0", Style::default().fg(DIM_TEXT)),
        ]),
        Line::from(Span::styled(
            "  Nostr Multi-Platform framework",
            Style::default().fg(DIM_TEXT),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  NMP runtime:  ", Style::default().fg(DIM_TEXT)),
            Span::styled(nmp_status.to_string(), Style::default().fg(BODY_TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Updates:      ", Style::default().fg(DIM_TEXT)),
            Span::styled(
                state.update_count.to_string(),
                Style::default().fg(BODY_TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Pending:      ", Style::default().fg(DIM_TEXT)),
            Span::styled(
                format!("{} actions", state.pending_actions.len()),
                Style::default().fg(BODY_TEXT),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn section_block(title: &'static str) -> Block<'static> {
    Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ))
}

fn status_dot_conn(connection_label: &str) -> (char, ratatui::style::Color) {
    let lower = connection_label.to_ascii_lowercase();
    if lower.contains("disconnect") || lower.contains("down") || lower.contains("fail") {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_cursor_fallback_is_zero() {
        let state = AppState::default();
        assert_eq!(settings_cursor(&state), 0);
    }

    #[test]
    fn section_names_count_is_six() {
        assert_eq!(SECTION_NAMES.len(), 6);
    }

    #[test]
    fn truncate_adds_ellipsis() {
        assert_eq!(truncate("hello world", 5), "hell\u{2026}");
    }

    #[test]
    fn truncate_passthrough_when_short() {
        assert_eq!(truncate("hi", 10), "hi");
    }
}
