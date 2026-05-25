use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use super::profile_wire::ProfileWire;

const PALETTE: [Color; 8] = [
    Color::Rgb(244, 114, 182),
    Color::Rgb(56, 189, 248),
    Color::Rgb(52, 211, 153),
    Color::Rgb(251, 191, 36),
    Color::Rgb(167, 139, 250),
    Color::Rgb(248, 113, 113),
    Color::Rgb(45, 212, 191),
    Color::Rgb(250, 204, 21),
];

/// Circular-ish terminal avatar with deterministic identicon fallback.
///
/// Terminals render cells, not real circles, so this widget uses a compact
/// bordered tile with profile initials and a stable pubkey-derived accent.
pub struct NostrAvatar<'a> {
    profile: &'a ProfileWire,
    border_style: Style,
}

impl<'a> NostrAvatar<'a> {
    pub fn new(profile: &'a ProfileWire) -> Self {
        Self {
            profile,
            border_style: Style::default().fg(accent_for(&profile.pubkey)),
        }
    }

    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }
}

impl Widget for NostrAvatar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let accent = accent_for(&self.profile.pubkey);
        let initials = self.profile.initials();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.border_style)
            .style(Style::default().bg(Color::Reset));
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.is_empty() {
            return;
        }

        let fill = Style::default()
            .fg(Color::Black)
            .bg(accent)
            .add_modifier(Modifier::BOLD);
        let line = Line::from(Span::styled(initials, fill));
        Paragraph::new(line)
            .alignment(Alignment::Center)
            .style(fill)
            .render(center_line(inner), buf);
    }
}

fn accent_for(pubkey: &str) -> Color {
    let hash = pubkey.bytes().fold(5381usize, |acc, byte| {
        ((acc << 5).wrapping_add(acc)) ^ byte as usize
    });
    PALETTE[hash % PALETTE.len()]
}

fn center_line(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1) / 2,
        width: area.width,
        height: 1,
    }
}
