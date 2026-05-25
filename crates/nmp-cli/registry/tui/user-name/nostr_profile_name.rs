use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::profile_wire::ProfileWire;

/// Inline profile display name with fallback to the Rust-formatted npub.
pub struct NostrProfileName<'a> {
    profile: &'a ProfileWire,
    style: Style,
}

impl<'a> NostrProfileName<'a> {
    pub fn new(profile: &'a ProfileWire) -> Self {
        Self {
            profile,
            style: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        }
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn line(&self) -> Line<'static> {
        Line::from(Span::styled(self.profile.display().to_string(), self.style))
    }
}

impl Widget for NostrProfileName<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.line()).render(area, buf);
    }
}
