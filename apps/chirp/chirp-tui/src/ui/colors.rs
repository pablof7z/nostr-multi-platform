//! Truecolor palette for the approach-b Home tab redesign.
//!
//! All constants live here so the Home rendering modules
//! (`home`, `post_list`, `post_detail`, `relay_panel`) stay visually consistent.

use ratatui::style::Color;

pub const HEADER_BG: Color = Color::Rgb(0x1a, 0x1a, 0x2e);
pub const FOOTER_BG: Color = Color::Rgb(0x12, 0x12, 0x22);
pub const DETAIL_BG: Color = Color::Rgb(0x10, 0x10, 0x1c);
pub const LIST_BG: Color = Color::Rgb(0x14, 0x14, 0x24);
pub const SELECTED_BG: Color = Color::Rgb(0x22, 0x2a, 0x55);
pub const ACCENT_CYAN: Color = Color::Rgb(0x55, 0xd0, 0xe0);
pub const DIM_TEXT: Color = Color::Rgb(0x88, 0x88, 0x99);
pub const DIMMER_TEXT: Color = Color::Rgb(0x55, 0x55, 0x66);
pub const BODY_TEXT: Color = Color::Rgb(0xee, 0xee, 0xee);
pub const HEART: Color = Color::Rgb(0xff, 0x5d, 0x5d);
pub const ZAP: Color = Color::Rgb(0xff, 0xd1, 0x3a);
pub const REPOST: Color = Color::Rgb(0x66, 0xe0, 0x86);
pub const REPLY_COLOR: Color = Color::Rgb(0x6a, 0xc8, 0xff);
pub const RELAY_OK: Color = Color::Rgb(0x55, 0xe0, 0x88);
pub const RELAY_DOWN: Color = Color::Rgb(0xff, 0x5d, 0x5d);
pub const RELAY_CONNECTING: Color = Color::Rgb(0xff, 0xd1, 0x3a);

pub const AUTHOR_CYCLE: [Color; 6] = [
    Color::Rgb(0xff, 0x8e, 0xc8),
    Color::Rgb(0x8e, 0xc8, 0xff),
    Color::Rgb(0xc8, 0xff, 0x8e),
    Color::Rgb(0xff, 0xc8, 0x8e),
    Color::Rgb(0xc8, 0x8e, 0xff),
    Color::Rgb(0x8e, 0xff, 0xe0),
];

pub fn author_color(pubkey: &str) -> Color {
    let b = pubkey.as_bytes().first().copied().unwrap_or(0) as usize;
    AUTHOR_CYCLE[b % AUTHOR_CYCLE.len()]
}

pub fn format_age(unix_ts: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let age = now.saturating_sub(unix_ts);
    if age < 60 {
        format!("{}s", age)
    } else if age < 3600 {
        format!("{}m", age / 60)
    } else if age < 86400 {
        format!("{}h", age / 3600)
    } else {
        format!("{}d", age / 86400)
    }
}

pub fn fmt_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
