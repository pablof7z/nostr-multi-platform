//! EmbedChromeContainer — the visual wrapper for any kind renderer (F-CR-06).
//!
//! Provides left border, indentation, and depth visual weight. Knows nothing
//! about the inner content. Matches ADR-0034's design.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

/// Simple chrome that draws a left border whose intensity can indicate depth.
pub struct EmbedChromeContainer {
    pub depth: u8,
    pub collapsed: bool,
}

impl EmbedChromeContainer {
    pub fn new(depth: u8, collapsed: bool) -> Self {
        Self { depth, collapsed }
    }

    /// Returns the inner area where the actual kind renderer should draw.
    pub fn inner(&self, area: Rect) -> Rect {
        if area.width <= 2 {
            return Rect::default();
        }
        Rect {
            x: area.x + 2,
            y: area.y,
            width: area.width - 2,
            height: area.height,
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 2 || area.height == 0 {
            return;
        }

        let border_style = if self.collapsed {
            Style::default().fg(Color::Rgb(100, 100, 110))
        } else {
            let g = 160u8.saturating_add(self.depth.saturating_mul(8));
            Style::default().fg(Color::Rgb(140, g.min(200), 220))
        };

        // Draw simple left border characters
        for y in area.y..area.bottom() {
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_char('│').set_style(border_style);
            }
            if let Some(cell) = buf.cell_mut((area.x + 1, y)) {
                cell.set_char(' ').set_style(border_style);
            }
        }
    }
}
