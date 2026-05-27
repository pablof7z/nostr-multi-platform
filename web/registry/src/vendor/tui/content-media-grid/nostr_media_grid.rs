use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use ratatui_image::{protocol::Protocol, Image};

/// Display-only media grid for terminal clients.
///
/// Fetching, decoding, and inline image protocol negotiation are host
/// capabilities. The widget consumes kernel-projected media URLs and optional
/// host-provided image protocols for those URLs.
pub struct NostrMediaGrid<'a> {
    urls: &'a [String],
    kind: &'a str,
    images: &'a [(&'a str, &'a Protocol)],
    style: Style,
}

impl<'a> NostrMediaGrid<'a> {
    pub fn new(urls: &'a [String], kind: &'a str) -> Self {
        Self {
            urls,
            kind,
            images: &[],
            style: Style::default().fg(Color::Rgb(186, 230, 253)),
        }
    }

    pub fn images(mut self, images: &'a [(&'a str, &'a Protocol)]) -> Self {
        self.images = images;
        self
    }

    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        if self.urls.is_empty() {
            return vec![Line::from(Span::styled("[media unavailable]", self.style))];
        }
        self.urls
            .iter()
            .enumerate()
            .map(|(idx, url)| self.line_for_url(idx, url, width))
            .collect()
    }

    pub fn preferred_height(&self) -> u16 {
        if self.urls.is_empty() {
            return 3;
        }
        if self.urls.iter().any(|url| self.image_for(url).is_some()) {
            rows_for(self.urls.len()).saturating_mul(12)
        } else {
            self.lines(80)
                .len()
                .saturating_add(2)
                .min(u16::MAX as usize) as u16
        }
    }
}

impl Widget for NostrMediaGrid<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(51, 65, 85)));
        let inner = block.inner(area);
        block.render(area, buf);

        if self.urls.iter().any(|url| self.image_for(url).is_some()) {
            self.render_image_cells(inner, buf);
        } else {
            Paragraph::new(self.lines(inner.width as usize)).render(inner, buf);
        }
    }
}

impl NostrMediaGrid<'_> {
    fn render_image_cells(&self, area: Rect, buf: &mut Buffer) {
        let count = self.urls.len();
        if count == 0 || area.is_empty() {
            return;
        }
        let cells = media_cells(area, count);
        for (idx, tile) in cells.into_iter().enumerate() {
            let Some(url) = self.urls.get(idx) else {
                continue;
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(14, 165, 233)));
            let inner = block.inner(tile);
            block.render(tile, buf);
            if let Some(protocol) = self.image_for(url) {
                Image::new(protocol).allow_clipping(true).render(inner, buf);
            } else {
                Paragraph::new(vec![self.line_for_url(idx, url, inner.width as usize)])
                    .style(self.style)
                    .render(inner, buf);
            }
        }
    }

    fn line_for_url(&self, idx: usize, url: &str, width: usize) -> Line<'static> {
        let label = format!(
            "[{} {}/{}] {}",
            self.kind.to_ascii_lowercase(),
            idx + 1,
            self.urls.len(),
            truncate(url, width.saturating_sub(16)),
        );
        Line::from(Span::styled(label, self.style))
    }

    fn image_for(&self, url: &str) -> Option<&Protocol> {
        self.images
            .iter()
            .find_map(|(candidate, image)| (*candidate == url).then_some(*image))
    }
}

fn media_cells(area: Rect, count: usize) -> Vec<Rect> {
    if count == 0 || area.is_empty() {
        return Vec::new();
    }
    if count == 1 {
        return vec![area];
    }

    let row_count = rows_for(count);
    let row_constraints = vec![Constraint::Ratio(1, row_count as u32); row_count as usize];
    let rows = Layout::vertical(row_constraints).split(area).to_vec();
    let mut cells = Vec::new();
    for (row_idx, row) in rows.into_iter().enumerate() {
        let remaining = count.saturating_sub(row_idx * 2);
        let columns = remaining.min(2);
        if columns == 1 {
            cells.push(row);
        } else {
            cells.extend(
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(row)
                    .iter()
                    .copied(),
            );
        }
    }
    cells.truncate(count);
    cells
}

fn rows_for(count: usize) -> u16 {
    count.div_ceil(2).max(1).min(u16::MAX as usize) as u16
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_include_every_media_url() {
        let urls = (0..6)
            .map(|idx| format!("https://example.com/{idx}.jpg"))
            .collect::<Vec<_>>();
        let lines = NostrMediaGrid::new(&urls, "image").lines(80);
        assert_eq!(lines.len(), urls.len());
    }

    #[test]
    fn media_cells_include_every_image() {
        let cells = media_cells(Rect::new(0, 0, 80, 36), 5);
        assert_eq!(cells.len(), 5);
        assert_eq!(rows_for(5), 3);
    }
}
