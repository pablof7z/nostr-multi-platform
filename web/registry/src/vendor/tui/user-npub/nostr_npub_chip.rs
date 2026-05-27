use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use super::profile_wire::ProfileWire;

/// Display-only npub chip.
///
/// Clipboard writes are a host capability. Keep this widget pure and handle
/// copy actions in the surrounding TUI input loop.
pub struct NostrNpubChip<'a> {
    profile: &'a ProfileWire,
    style: Style,
}

impl<'a> NostrNpubChip<'a> {
    pub fn new(profile: &'a ProfileWire) -> Self {
        Self {
            profile,
            style: Style::default()
                .fg(Color::Rgb(186, 230, 253))
                .bg(Color::Rgb(15, 23, 42)),
        }
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn label(&self) -> String {
        format!(" {} ", self.profile.npub_short)
    }
}

impl Widget for NostrNpubChip<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.style)
            .style(self.style);
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(Line::from(Span::styled(self.label(), self.style))).render(inner, buf);
    }
}
