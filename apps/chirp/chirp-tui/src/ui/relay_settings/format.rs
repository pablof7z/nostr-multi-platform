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

/// Title-case a raw lowercase token for display (e.g. `"connected"` →
/// `"Connected"`). The kernel emits raw tokens; presentation formatting lives
/// in the render layer. A `"—"` em-dash sentinel (used for "no auth") passes
/// through unchanged.
pub(super) fn title_case(value: &str) -> String {
    if value == "\u{2014}" {
        return value.to_string();
    }
    value
        .split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>()
                        + &chars.as_str().to_ascii_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Compact integer formatter for event counts: `1234` → `"1.2K"`,
/// `1_500_000` → `"1.5M"`, small values pass through unchanged. Mirrors the
/// former kernel `compact_count`: whole magnitudes drop the decimal (`1K`,
/// not `1.0K`) so the rendered text matches the iOS / Android shells.
pub(super) fn compact_count(n: u64) -> String {
    let magnitude = |v: f64, suffix: char| -> String {
        if v.fract() == 0.0 {
            format!("{}{suffix}", v as u64)
        } else {
            format!("{v:.1}{suffix}")
        }
    };
    if n >= 1_000_000 {
        magnitude(n as f64 / 1_000_000.0, 'M')
    } else if n >= 1_000 {
        magnitude(n as f64 / 1_000.0, 'K')
    } else {
        n.to_string()
    }
}

/// Human byte-size formatter: `0` → `"0 B"`, `1536` → `"1.5 KB"`,
/// scaling through KB / MB / GB.
pub(super) fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let f = n as f64;
    if f >= GB {
        format!("{:.1} GB", f / GB)
    } else if f >= MB {
        format!("{:.1} MB", f / MB)
    } else if f >= KB {
        format!("{:.1} KB", f / KB)
    } else {
        format!("{n} B")
    }
}

/// Consumer-count phrase: `0` → `""`, `1` → `"1 consumer"`,
/// `N` → `"N consumers"`.
pub(super) fn consumer_count_label(n: u32) -> String {
    match n {
        0 => String::new(),
        1 => "1 consumer".to_string(),
        _ => format!("{n} consumers"),
    }
}

/// Short wire id: ≤12 chars passes through whole; longer is truncated to the
/// first 8 chars plus an ellipsis.
pub(super) fn short_wire_id(wire_id: &str) -> String {
    if wire_id.chars().count() <= 12 {
        wire_id.to_string()
    } else {
        let head: String = wire_id.chars().take(8).collect();
        format!("{head}\u{2026}")
    }
}

/// Format raw discovery kind numbers for display, replicating the label
/// mapping the kernel projection used to embed: each kind as `"label (kind)"`
/// joined by `", "`. Empty list → `"none"`.
pub(super) fn discovery_kinds_label(kinds: &[u64]) -> String {
    if kinds.is_empty() {
        return "none".to_string();
    }
    kinds
        .iter()
        .map(|&kind| {
            let label = match kind {
                0 => "profile",
                3 => "follows",
                10002 => "relay-list",
                _ => "list",
            };
            format!("{label} ({kind})")
        })
        .collect::<Vec<_>>()
        .join(", ")
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
