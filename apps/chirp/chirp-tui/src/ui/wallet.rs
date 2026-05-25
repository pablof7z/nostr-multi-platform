//! Approach-b Wallet tab: single full-width pane showing connection status and balance.
//!
//! Connected state shows balance, relay URL, wallet npub, and key hints.
//! Disconnected state prompts the user to connect a NWC wallet.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AppState;
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIM_TEXT, DIMMER_TEXT, RELAY_CONNECTING, RELAY_DOWN,
    RELAY_OK, ZAP, fmt_count,
};

pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let wallet = &state.features.wallet;
    let connected = is_connected(wallet);

    let title_spans: Vec<Span<'static>> = if connected {
        let (dot, dot_color) = status_dot(&wallet.status);
        vec![
            Span::styled(
                " Wallet ",
                Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{dot} {} ", connected_label(&wallet.status)),
                Style::default().fg(dot_color),
            ),
        ]
    } else {
        vec![Span::styled(
            " Wallet ",
            Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        )]
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG))
        .title(Line::from(title_spans));

    let lines = if connected {
        build_connected_lines(wallet)
    } else {
        build_disconnected_lines()
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    f.render_widget(paragraph, area);
}

fn is_connected(wallet: &crate::feature_snapshot::WalletLine) -> bool {
    !wallet.relay_url.is_empty()
        || wallet.status.to_ascii_lowercase().contains("connected")
}

fn build_connected_lines(
    wallet: &crate::feature_snapshot::WalletLine,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = vec![Line::from("")];

    // Balance row
    match wallet.balance_msats {
        Some(msats) => {
            let sats = msats / 1000;
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    fmt_count(sats),
                    Style::default().fg(ZAP).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("sats", Style::default().fg(DIM_TEXT)),
            ]));
        }
        None => {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled("—", Style::default().fg(DIM_TEXT)),
                Span::raw("  "),
                Span::styled("sats", Style::default().fg(DIM_TEXT)),
            ]));
        }
    }

    lines.push(Line::from(""));

    // Relay URL
    let relay_display = if wallet.relay_url.is_empty() {
        "—".to_string()
    } else {
        wallet.relay_url.clone()
    };
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled("Relay   ", Style::default().fg(DIM_TEXT)),
        Span::styled(relay_display, Style::default().fg(BODY_TEXT)),
    ]));

    // Wallet npub
    let npub_display = if wallet.wallet_npub.is_empty() {
        "—".to_string()
    } else {
        wallet.wallet_npub.clone()
    };
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled("Wallet  ", Style::default().fg(DIM_TEXT)),
        Span::styled(npub_display, Style::default().fg(DIM_TEXT)),
    ]));

    lines.push(Line::from(""));

    // Key hints
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled("[p]", Style::default().fg(ACCENT_CYAN)),
        Span::styled(" Pay invoice    ", Style::default().fg(BODY_TEXT)),
        Span::styled("[n]", Style::default().fg(ACCENT_CYAN)),
        Span::styled(" Connect new    ", Style::default().fg(BODY_TEXT)),
        Span::styled("[d]", Style::default().fg(ACCENT_CYAN)),
        Span::styled(" Disconnect", Style::default().fg(BODY_TEXT)),
    ]));

    lines.push(Line::from(""));
    lines
}

fn build_disconnected_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            "   No wallet connected.",
            Style::default().fg(DIMMER_TEXT),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("   Press  "),
            Span::styled("n", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
            Span::raw("  to connect a NWC wallet"),
        ]),
        Line::from(vec![
            Span::raw("   Press  "),
            Span::styled("?", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
            Span::raw("  for help"),
        ]),
        Line::from(""),
    ]
}

fn status_dot(status: &str) -> (char, ratatui::style::Color) {
    let lower = status.to_ascii_lowercase();
    if lower.contains("disconnect") || lower.contains("down") || lower.contains("fail") || lower.is_empty() {
        ('\u{25cb}', RELAY_DOWN) // ○
    } else if lower.contains("connect") || lower == "open" || lower.contains("active") {
        ('\u{25cf}', RELAY_OK) // ●
    } else {
        ('\u{25cc}', RELAY_CONNECTING) // ◌
    }
}

fn connected_label(status: &str) -> String {
    if status.is_empty() {
        return "connected".to_string();
    }
    let mut chars = status.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature_snapshot::WalletLine;

    #[test]
    fn disconnected_when_relay_url_empty_and_status_empty() {
        let wallet = WalletLine {
            status: String::new(),
            relay_url: String::new(),
            wallet_npub: String::new(),
            balance_msats: None,
        };
        assert!(!is_connected(&wallet));
    }

    #[test]
    fn connected_when_relay_url_present() {
        let wallet = WalletLine {
            status: String::new(),
            relay_url: "nos.lol".to_string(),
            wallet_npub: String::new(),
            balance_msats: None,
        };
        assert!(is_connected(&wallet));
    }

    #[test]
    fn connected_when_status_contains_connected() {
        let wallet = WalletLine {
            status: "connected".to_string(),
            relay_url: String::new(),
            wallet_npub: String::new(),
            balance_msats: None,
        };
        assert!(is_connected(&wallet));
    }

    #[test]
    fn balance_line_uses_fmt_count() {
        let wallet = WalletLine {
            status: "connected".to_string(),
            relay_url: "nos.lol".to_string(),
            wallet_npub: String::new(),
            balance_msats: Some(123_456_000),
        };
        let lines = build_connected_lines(&wallet);
        // Balance row is index 1 (after blank line at 0)
        let balance_line = &lines[1];
        let text: String = balance_line.spans.iter().map(|s| s.content.as_ref()).collect();
        // fmt_count(123456) => "123.5k"
        assert!(text.contains("123.5k"), "got: {text}");
    }

    #[test]
    fn balance_line_shows_dash_when_none() {
        let wallet = WalletLine {
            status: "connected".to_string(),
            relay_url: "nos.lol".to_string(),
            wallet_npub: String::new(),
            balance_msats: None,
        };
        let lines = build_connected_lines(&wallet);
        let balance_line = &lines[1];
        let text: String = balance_line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('—'), "got: {text}");
    }
}
