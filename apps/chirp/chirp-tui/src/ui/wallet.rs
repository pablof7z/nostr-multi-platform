//! Approach-b Wallet tab: single rich pane showing connection status and balance.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::colors::{
    ACCENT_CYAN, BODY_TEXT, DETAIL_BG, DIMMER_TEXT, DIM_TEXT, RELAY_CONNECTING, RELAY_DOWN,
    RELAY_OK, ZAP,
};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let wallet = &state.features.wallet;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT_CYAN))
        .style(Style::default().bg(DETAIL_BG))
        .title(Span::styled(
            " Wallet ",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ));

    let lines = build_lines(wallet);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(DETAIL_BG).fg(BODY_TEXT));
    frame.render_widget(paragraph, area);
}

fn build_lines(wallet: &crate::feature_snapshot::WalletLine) -> Vec<Line<'static>> {
    let (dot, dot_color) = status_dot(&wallet.status);
    let status_label: String = if wallet.status.is_empty() {
        "Disconnected".to_string()
    } else {
        // Capitalize first letter
        let mut chars = wallet.status.chars();
        match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        }
    };

    let relay_display: String = if wallet.relay_url.is_empty() {
        "—".to_string()
    } else {
        wallet.relay_url.clone()
    };
    let npub_display: String = if wallet.wallet_npub.is_empty() {
        "—".to_string()
    } else {
        wallet.wallet_npub.clone()
    };

    let mut lines: Vec<Line<'static>> = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("   "),
            Span::styled(dot.to_string(), Style::default().fg(dot_color)),
            Span::raw(" "),
            Span::styled(
                status_label,
                Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
    ];

    // Balance row
    match wallet.balance_msats {
        Some(msats) => {
            let sats = msats / 1000;
            let sats_str = format_sats(sats);
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled("Balance   ", Style::default().fg(DIM_TEXT)),
                Span::styled(
                    sats_str,
                    Style::default().fg(ZAP).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled("sats", Style::default().fg(DIMMER_TEXT)),
            ]));
        }
        None => {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled("Balance   ", Style::default().fg(DIM_TEXT)),
                Span::styled("balance unavailable", Style::default().fg(DIMMER_TEXT)),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled("Relay     ", Style::default().fg(DIM_TEXT)),
        Span::styled(relay_display, Style::default().fg(BODY_TEXT)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled("Wallet    ", Style::default().fg(DIM_TEXT)),
        Span::styled(npub_display, Style::default().fg(DIMMER_TEXT)),
    ]));
    lines.push(Line::from(""));

    lines
}

fn status_dot(status: &str) -> (char, ratatui::style::Color) {
    let lower = status.to_ascii_lowercase();
    if lower.contains("disconnected")
        || lower.contains("down")
        || lower.contains("failed")
        || lower.is_empty()
    {
        ('\u{25cb}', RELAY_DOWN) // ○
    } else if lower.contains("connected") || lower == "open" || lower.contains("active") {
        ('\u{25cf}', RELAY_OK) // ●
    } else {
        ('\u{25cc}', RELAY_CONNECTING) // ◌
    }
}

fn format_sats(n: u64) -> String {
    // Format with comma thousands separator
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_sats_adds_commas() {
        assert_eq!(format_sats(0), "0");
        assert_eq!(format_sats(999), "999");
        assert_eq!(format_sats(1000), "1,000");
        assert_eq!(format_sats(12450), "12,450");
        assert_eq!(format_sats(1_000_000), "1,000,000");
    }
}
