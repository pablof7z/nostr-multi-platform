use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

/// Display-only media grid placeholder for terminal clients.
///
/// Fetching, decoding, and inline image protocol negotiation are host
/// capabilities. This widget renders the kernel-projected media URLs in a
/// stable, inspectable grid-shaped summary.
pub struct NostrMediaGrid<'a> {
    urls: &'a [String],
    kind: &'a str,
    style: Style,
}

impl<'a> NostrMediaGrid<'a> {
    pub fn new(urls: &'a [String], kind: &'a str) -> Self {
        Self {
            urls,
            kind,
            style: Style::default().fg(Color::Rgb(186, 230, 253)),
        }
    }

    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        if self.urls.is_empty() {
            return vec![Line::from(Span::styled("[media unavailable]", self.style))];
        }
        self.urls
            .iter()
            .take(4)
            .enumerate()
            .map(|(idx, url)| {
                let more = if idx == 3 && self.urls.len() > 4 {
                    format!(" +{} more", self.urls.len() - 4)
                } else {
                    String::new()
                };
                let label = format!(
                    "[{} {}/{}] {}{}",
                    self.kind.to_ascii_lowercase(),
                    idx + 1,
                    self.urls.len(),
                    truncate(url, width.saturating_sub(16)),
                    more
                );
                Line::from(Span::styled(label, self.style))
            })
            .collect()
    }
}

impl Widget for NostrMediaGrid<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(51, 65, 85)));
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(self.lines(inner.width as usize)).render(inner, buf);
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let mut out = value
            .chars()
            .take(max.saturating_sub(1))
            .collect::<String>();
        out.push('…');
        out
    }
}
