//! Transient toast notification stack.
//!
//! Reads `state.toasts: Vec<(String, u8)>` once the wiring agent adds the
//! field. Until then, `toasts_of` returns an empty vec and the widget is
//! simply a no-op.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::colors::{ACCENT_CYAN, DIM_TEXT, DIMMER_TEXT, HEART, REPOST, ZAP};

/// Extract toasts from state.
fn toasts_of(state: &AppState) -> Vec<(String, u8)> {
    state.toasts.clone()
}

/// Choose foreground color based on toast message prefix and TTL.
fn toast_color(msg: &str, ttl: u8) -> ratatui::style::Color {
    // Fade near expiry
    if ttl < 10 {
        return DIMMER_TEXT;
    }
    if ttl < 20 {
        return DIM_TEXT;
    }
    // Full color by prefix
    if msg.starts_with('\u{2713}') || msg.starts_with("✓") {
        REPOST
    } else if msg.starts_with('\u{26a0}') || msg.starts_with("⚠") {
        ZAP
    } else if msg.starts_with('\u{2717}') || msg.starts_with("✗") {
        HEART
    } else {
        ACCENT_CYAN
    }
}

/// Approximate seconds remaining from TTL (50 ticks ≈ 5 s at ~10 Hz).
fn ttl_label(ttl: u8) -> String {
    let secs = (ttl as u16 * 10 / 50).max(1);
    format!("[{}s]", secs)
}

/// Render toast stack.
///
/// `area` should be positioned above the status bar with height equal to the
/// number of toasts to display (caller clips to `min(3, toasts.len())`).
pub fn render(f: &mut Frame, area: Rect, state: &AppState) {
    let toasts = toasts_of(state);
    if toasts.is_empty() || area.height == 0 {
        return;
    }

    // Most-recent toasts first, cap at 3.
    let visible: Vec<_> = toasts.iter().rev().take(3).collect();
    let total_width = area.width as usize;

    for (i, (msg, ttl)) in visible.iter().enumerate() {
        let row = area.y + i as u16;
        if row >= area.y + area.height {
            break;
        }
        let row_area = Rect {
            y: row,
            height: 1,
            ..area
        };

        let color = toast_color(msg, *ttl);
        let age = ttl_label(*ttl);
        // Right-align age label; left-align message with leading padding.
        let prefix = "  ";
        let msg_max = total_width.saturating_sub(age.len() + 2);
        let msg_display: String = if prefix.len() + msg.len() > msg_max {
            format!("{}{}", prefix, &msg[..msg_max.saturating_sub(prefix.len())])
        } else {
            format!("{}{}", prefix, msg)
        };

        let pad = total_width
            .saturating_sub(msg_display.len() + age.len() + 2);
        let line = Line::from(vec![
            Span::styled(msg_display, Style::default().fg(color)),
            Span::raw(" ".repeat(pad)),
            Span::styled(format!("  {}", age), Style::default().fg(DIM_TEXT)),
        ]);
        f.render_widget(Paragraph::new(line), row_area);
    }
}
