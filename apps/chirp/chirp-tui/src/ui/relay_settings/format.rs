//! Pure string / line formatting helpers for the relay-settings panes.
//!
//! Split out of `relay_settings.rs` to keep that file under the AGENTS.md
//! 500-LOC hard ceiling. Everything here is presentation-only (no snapshot
//! shape, no app state): label/wrap line builders, the connection status dot,
//! short-URL trimming, render-time relative-time formatting, and small string
//! utilities.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::ui::colors::{BODY_TEXT, DIM_TEXT, RELAY_CONNECTING, RELAY_DOWN, RELAY_OK};

pub(super) fn label_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(DIM_TEXT)),
        Span::styled(value.to_string(), Style::default().fg(BODY_TEXT)),
    ])
}

pub(super) fn append_wrapped(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    value: &str,
    pane_width: usize,
) {
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

pub(super) fn status_dot(connection_label: &str) -> (char, ratatui::style::Color) {
    let lower = connection_label.to_ascii_lowercase();
    if lower.contains("disconnected") || lower.contains("down") || lower.contains("failed") {
        ('\u{25cb}', RELAY_DOWN)
    } else if lower.contains("connected") || lower == "open" {
        ('\u{25cf}', RELAY_OK)
    } else {
        ('\u{25cc}', RELAY_CONNECTING)
    }
}

pub(super) fn short_relay_url(url: &str) -> String {
    url.strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_string()
}

/// Format a Unix-epoch-millisecond timestamp as a human-relative string at
/// render time (aim.md §62: the projection carries raw `*_ms` and the shell
/// formats here). Returns "never" when `ms == 0` (the "never observed"
/// sentinel). This is the TUI peer of iOS `relativeTimeFromUnixSeconds` /
/// Android `formatRelativeTime`.
pub(super) fn format_ms_ago(ms: u64) -> String {
    if ms == 0 {
        return "never".to_string();
    }
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let then_secs = ms / 1_000;
    nmp_core::display::format_ago_secs(now_secs, then_secs)
}

pub(super) fn empty_dash(value: &str) -> String {
    if value.is_empty() {
        "-".to_string()
    } else {
        value.to_string()
    }
}

pub(super) fn truncate(value: &str, max: usize) -> String {
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
