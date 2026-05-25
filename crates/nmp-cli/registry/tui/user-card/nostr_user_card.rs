use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};
use ratatui_image::protocol::Protocol;

use super::{
    nostr_avatar::NostrAvatar, nostr_nip05_badge::NostrNip05Badge,
    nostr_profile_name::NostrProfileName, profile_wire::ProfileWire,
};

/// Compact author header: avatar, display name, and optional NIP-05 badge.
pub struct NostrUserCard<'a> {
    profile: &'a ProfileWire,
    avatar_image: Option<&'a Protocol>,
    style: Style,
}

impl<'a> NostrUserCard<'a> {
    pub fn new(profile: &'a ProfileWire) -> Self {
        Self {
            profile,
            avatar_image: None,
            style: Style::default().fg(Color::White).bg(Color::Rgb(12, 16, 28)),
        }
    }

    pub fn avatar_image(mut self, image: Option<&'a Protocol>) -> Self {
        self.avatar_image = image;
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl Widget for NostrUserCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(30, 41, 59)))
            .style(self.style);
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.is_empty() {
            return;
        }

        let chunks = Layout::horizontal([Constraint::Length(14), Constraint::Min(0)]).split(inner);
        NostrAvatar::new(self.profile)
            .image(self.avatar_image)
            .render(chunks[0], buf);

        let text_area = inset(chunks[1], 1, 0);
        let name = NostrProfileName::new(self.profile).line();
        let badge = NostrNip05Badge::from_profile(self.profile)
            .map(|badge| badge.line())
            .unwrap_or_else(|| Line::from(""));

        Paragraph::new(vec![name, badge])
            .style(self.style)
            .render(text_area, buf);
    }
}

fn inset(area: Rect, x: u16, y: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(x),
        y: area.y.saturating_add(y),
        width: area.width.saturating_sub(x),
        height: area.height.saturating_sub(y),
    }
}
