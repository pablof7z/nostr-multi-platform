#[path = "../../../../../../crates/nmp-cli/registry/tui/user-name/nostr_profile_name.rs"]
pub mod nostr_profile_name;
#[path = "../../../../../../crates/nmp-cli/registry/tui/user-core/profile_wire.rs"]
pub mod profile_wire;

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use nostr_profile_name::NostrProfileName;
use profile_wire::ProfileWire;

pub fn profile_name_span(
    profile: &ProfileWire,
    style: Style,
    max_width: usize,
) -> (Span<'static>, usize) {
    let line = NostrProfileName::new(profile).style(style).line();
    let text = line_text(line);
    let truncated = truncate(&text, max_width);
    let width = truncated.chars().count();
    (Span::styled(truncated, style), width)
}

fn line_text(line: Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
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
