use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::profile_wire::ProfileWire;

/// NIP-05 verified identity badge.
pub struct NostrNip05Badge {
    label: String,
    style: Style,
}

impl NostrNip05Badge {
    pub fn new(nip05: &str) -> Option<Self> {
        Some(Self {
            label: display_nip05(nip05)?,
            style: Style::default().fg(Color::Rgb(45, 212, 191)),
        })
    }

    pub fn from_profile(profile: &ProfileWire) -> Option<Self> {
        Self::new(profile.nip05()?)
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn line(&self) -> Line<'static> {
        Line::from(vec![
            Span::styled("✓ ", self.style.add_modifier(Modifier::BOLD)),
            Span::styled(self.label.clone(), self.style),
        ])
    }
}

impl Widget for NostrNip05Badge {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.line()).render(area, buf);
    }
}

fn display_nip05(nip05: &str) -> Option<String> {
    let trimmed = nip05.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.strip_prefix("_@") {
        Some(domain) if !domain.is_empty() => Some(domain.to_string()),
        _ => Some(trimmed.to_string()),
    }
}
