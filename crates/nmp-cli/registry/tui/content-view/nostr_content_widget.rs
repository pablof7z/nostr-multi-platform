use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget, Wrap},
};

use super::super::{
    content_kind_registry::EmbeddedEvent,
    content_tree_wire::{WireNode, WireUri},
    nostr_media_grid::NostrMediaGrid,
    nostr_quote_card::NostrQuoteCard,
    ratatui_text_wrap::wrap_spans,
};
use super::{is_inline_root, NostrContentView};

impl Widget for NostrContentView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut cursor = area.y;
        let mut root_pos = 0usize;
        while root_pos < self.tree.roots.len() {
            let root = self.tree.roots[root_pos];
            let Some(node) = self.tree.node(root) else {
                root_pos += 1;
                continue;
            };
            if is_inline_root(node) {
                let inline = self.collect_inline_roots(&mut root_pos);
                self.render_lines(
                    wrap_spans(inline, area.width as usize),
                    area,
                    buf,
                    &mut cursor,
                );
            } else {
                self.render_node(root, area, buf, &mut cursor);
                root_pos += 1;
            }
            if cursor >= area.bottom() {
                break;
            }
        }
        if cursor == area.y {
            Paragraph::new("").render(area, buf);
        }
    }
}

impl NostrContentView<'_> {
    fn render_node(&self, index: usize, area: Rect, buf: &mut Buffer, cursor: &mut u16) {
        let Some(node) = self.tree.node(index) else {
            return;
        };
        match node {
            WireNode::Paragraph { children } => self.render_paragraph(children, area, buf, cursor),
            WireNode::Media { urls, kind } => self.render_media(urls, kind, area, buf, cursor),
            WireNode::Image { src, .. } => {
                if let Some(src) = src {
                    self.render_media(std::slice::from_ref(src), "image", area, buf, cursor);
                }
            }
            WireNode::EventRef(uri) => {
                if !self.render_embedded_event(uri, area, buf, cursor) {
                    self.render_quote(node, area, buf, cursor);
                }
            }
            WireNode::BlockQuote { .. } => {
                self.render_quote(node, area, buf, cursor);
            }
            _ => {
                let lines = {
                    let mut out = Vec::new();
                    self.append_node(index, area.width as usize, &mut out);
                    out
                };
                self.render_lines(lines, area, buf, cursor);
            }
        }
    }

    fn render_paragraph(&self, children: &[usize], area: Rect, buf: &mut Buffer, cursor: &mut u16) {
        let mut inline = Vec::new();
        for child in children {
            let Some(node) = self.tree.node(*child) else {
                continue;
            };
            match node {
                WireNode::EventRef(uri) => {
                    self.render_lines(
                        wrap_spans(std::mem::take(&mut inline), area.width as usize),
                        area,
                        buf,
                        cursor,
                    );
                    if !self.render_embedded_event(uri, area, buf, cursor) {
                        self.render_node(*child, area, buf, cursor);
                    }
                }
                WireNode::Media { .. } | WireNode::Image { .. } => {
                    self.render_lines(
                        wrap_spans(std::mem::take(&mut inline), area.width as usize),
                        area,
                        buf,
                        cursor,
                    );
                    self.render_node(*child, area, buf, cursor);
                }
                _ => self.append_inline_node(*child, &mut inline),
            }
        }
        self.render_lines(wrap_spans(inline, area.width as usize), area, buf, cursor);
    }

    fn render_embedded_event(
        &self,
        uri: &WireUri,
        area: Rect,
        buf: &mut Buffer,
        cursor: &mut u16,
    ) -> bool {
        let Some(registry) = self.kind_registry else {
            return false;
        };
        let Some(envelope) = self.envelope_for(uri) else {
            return false;
        };
        let widget = EmbeddedEvent::new(envelope, registry);
        let height = widget.preferred_height(area.width);
        let rect = take_area(area, cursor, height);
        if rect.is_empty() {
            return true;
        }
        widget.render(rect, buf);
        *cursor = rect.bottom().saturating_add(1).min(area.bottom());
        true
    }

    fn envelope_for(
        &self,
        uri: &WireUri,
    ) -> Option<&nmp_content::embed_projection::EmbeddedEventEnvelope> {
        let events = self.embedded_events?;
        events.get(&uri.primary_id).or_else(|| events.get(&uri.uri))
    }

    fn render_quote(&self, node: &WireNode, area: Rect, buf: &mut Buffer, cursor: &mut u16) {
        let card = NostrQuoteCard::new(self.tree, node)
            .render_data(self.render_data)
            .media_images(self.media_images);
        let height = card.preferred_height(area.width as usize);
        let rect = take_area(area, cursor, height);
        if rect.is_empty() {
            return;
        }
        card.render(rect, buf);
        *cursor = rect.bottom().saturating_add(1).min(area.bottom());
    }

    fn render_media(
        &self,
        urls: &[String],
        kind: &str,
        area: Rect,
        buf: &mut Buffer,
        cursor: &mut u16,
    ) {
        let grid = NostrMediaGrid::new(urls, kind).images(self.media_images);
        let rect = take_area(area, cursor, grid.preferred_height());
        if rect.is_empty() {
            return;
        }
        grid.render(rect, buf);
        *cursor = rect.bottom().saturating_add(1).min(area.bottom());
    }

    fn render_lines(
        &self,
        lines: Vec<Line<'static>>,
        area: Rect,
        buf: &mut Buffer,
        cursor: &mut u16,
    ) {
        let lines = lines
            .into_iter()
            .filter(|line| line.spans.iter().any(|span| !span.content.is_empty()))
            .collect::<Vec<_>>();
        if lines.is_empty() {
            return;
        }
        let rect = take_area(area, cursor, lines.len() as u16);
        if rect.is_empty() {
            return;
        }
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(rect, buf);
        *cursor = rect.bottom();
    }
}

fn take_area(area: Rect, cursor: &mut u16, wanted_height: u16) -> Rect {
    if *cursor >= area.bottom() || wanted_height == 0 {
        return Rect::new(area.x, area.bottom(), area.width, 0);
    }
    let available = area.bottom().saturating_sub(*cursor);
    Rect {
        x: area.x,
        y: *cursor,
        width: area.width,
        height: wanted_height.min(available),
    }
}
