use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use ratatui_image::protocol::Protocol;

use super::{
    content_render_data::{ContentEventRenderData, ContentRenderData},
    content_tree_wire::{ContentTreeWire, WireNode},
    nostr_media_grid::NostrMediaGrid,
    ratatui_text_wrap::wrap_prefixed,
};

/// Terminal quote/event reference card.
pub struct NostrQuoteCard<'a> {
    tree: &'a ContentTreeWire,
    node: &'a WireNode,
    render_data: Option<&'a ContentRenderData>,
    media_images: &'a [(&'a str, &'a Protocol)],
    style: Style,
}

impl<'a> NostrQuoteCard<'a> {
    pub fn new(tree: &'a ContentTreeWire, node: &'a WireNode) -> Self {
        Self {
            tree,
            node,
            render_data: None,
            media_images: &[],
            style: Style::default().fg(Color::Rgb(203, 213, 225)),
        }
    }

    pub fn render_data(mut self, render_data: Option<&'a ContentRenderData>) -> Self {
        self.render_data = render_data;
        self
    }

    pub fn media_images(mut self, images: &'a [(&'a str, &'a Protocol)]) -> Self {
        self.media_images = images;
        self
    }

    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        match self.node {
            WireNode::EventRef(uri) => {
                if let Some(event) = self.render_data.and_then(|data| data.event_for(uri)) {
                    let header = format!("quote {} · kind:{}", event.author_label(), event.kind);
                    let mut lines = vec![Line::from(Span::styled(
                        header,
                        Style::default().fg(Color::Rgb(148, 163, 184)),
                    ))];
                    let body = if event.content_preview.is_empty() {
                        short_id(&event.id)
                    } else {
                        event.content_preview.clone()
                    };
                    lines.extend(wrap_prefixed(&body, width, "  ", muted_style()));
                    lines
                } else {
                    vec![Line::from(vec![
                        Span::styled("quote ", Style::default().fg(Color::Rgb(148, 163, 184))),
                        Span::styled(short_id(&uri.primary_id), self.style),
                    ])]
                }
            }
            WireNode::BlockQuote { children } => self
                .text_for_nodes(children)
                .split('\n')
                .flat_map(|line| wrap_prefixed(line, width, "> ", muted_style()))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn preferred_height(&self, width: usize) -> u16 {
        match self.node {
            WireNode::EventRef(uri) => self
                .render_data
                .and_then(|data| data.event_for(uri))
                .map(|event| 1 + self.event_body_height(event, width.saturating_sub(2)))
                .unwrap_or_else(|| self.lines(width).len() as u16),
            _ => self.lines(width).len() as u16,
        }
        .max(1)
    }

    fn text_for_nodes(&self, children: &[usize]) -> String {
        children
            .iter()
            .filter_map(|idx| self.tree.node(*idx))
            .map(|node| node.inline_label(self.tree))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Widget for NostrQuoteCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(Color::Rgb(71, 85, 105)));
        let inner = block.inner(area);
        block.render(area, buf);
        match self.node {
            WireNode::EventRef(uri) => {
                if let Some(event) = self.render_data.and_then(|data| data.event_for(uri)) {
                    self.render_event(event, inner, buf);
                } else {
                    Paragraph::new(self.lines(inner.width as usize)).render(inner, buf);
                }
            }
            _ => Paragraph::new(self.lines(inner.width as usize)).render(inner, buf),
        }
    }
}

impl NostrQuoteCard<'_> {
    fn render_event(&self, event: &ContentEventRenderData, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let header = format!("quote {} · kind:{}", event.author_label(), event.kind);
        Paragraph::new(Line::from(Span::styled(header, muted_style())))
            .render(Rect { height: 1, ..area }, buf);

        let mut cursor = area.y.saturating_add(1);
        let body_area = Rect {
            x: area.x.saturating_add(2),
            y: cursor,
            width: area.width.saturating_sub(2),
            height: area.bottom().saturating_sub(cursor),
        };
        if let Some(tree) = event.content_tree.as_ref() {
            self.render_tree(tree, body_area, buf, &mut cursor);
        } else {
            let body = if event.content_preview.is_empty() {
                short_id(&event.id)
            } else {
                event.content_preview.clone()
            };
            render_lines(
                wrap_prefixed(&body, body_area.width as usize, "", muted_style()),
                body_area,
                buf,
                &mut cursor,
            );
        }
    }

    fn render_tree(&self, tree: &ContentTreeWire, area: Rect, buf: &mut Buffer, cursor: &mut u16) {
        for root in &tree.roots {
            self.render_tree_node(tree, *root, area, buf, cursor);
            if *cursor >= area.bottom() {
                break;
            }
        }
    }

    fn render_tree_node(
        &self,
        tree: &ContentTreeWire,
        index: usize,
        area: Rect,
        buf: &mut Buffer,
        cursor: &mut u16,
    ) {
        let Some(node) = tree.node(index) else {
            return;
        };
        match node {
            WireNode::Paragraph { children } => {
                let mut text = String::new();
                for child in children {
                    let Some(child_node) = tree.node(*child) else {
                        continue;
                    };
                    match child_node {
                        WireNode::Media { urls, kind } => {
                            render_plain(&mut text, area, buf, cursor);
                            self.render_media(urls, kind, area, buf, cursor);
                        }
                        WireNode::Image { src, .. } => {
                            render_plain(&mut text, area, buf, cursor);
                            if let Some(src) = src {
                                self.render_media(
                                    std::slice::from_ref(src),
                                    "image",
                                    area,
                                    buf,
                                    cursor,
                                );
                            }
                        }
                        _ => text.push_str(&child_node.inline_label(tree)),
                    }
                }
                render_plain(&mut text, area, buf, cursor);
            }
            WireNode::Media { urls, kind } => self.render_media(urls, kind, area, buf, cursor),
            WireNode::Image { src, .. } => {
                if let Some(src) = src {
                    self.render_media(std::slice::from_ref(src), "image", area, buf, cursor);
                }
            }
            _ => {
                let mut text = node.inline_label(tree);
                render_plain(&mut text, area, buf, cursor);
            }
        }
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

    fn event_body_height(&self, event: &ContentEventRenderData, width: usize) -> u16 {
        if let Some(tree) = event.content_tree.as_ref() {
            tree.roots
                .iter()
                .filter_map(|root| tree.node(*root))
                .map(|node| self.node_height(tree, node, width))
                .sum::<u16>()
                .max(1)
        } else {
            wrap_prefixed(&event.content_preview, width, "", muted_style()).len() as u16
        }
    }

    fn node_height(&self, tree: &ContentTreeWire, node: &WireNode, width: usize) -> u16 {
        match node {
            WireNode::Media { urls, kind } => NostrMediaGrid::new(urls, kind)
                .images(self.media_images)
                .preferred_height()
                .saturating_add(1),
            WireNode::Image { src, .. } => src
                .as_ref()
                .map(|src| {
                    NostrMediaGrid::new(std::slice::from_ref(src), "image")
                        .images(self.media_images)
                        .preferred_height()
                        .saturating_add(1)
                })
                .unwrap_or(0),
            _ => wrap_prefixed(&node.inline_label(tree), width, "", muted_style()).len() as u16,
        }
    }
}

fn render_plain(text: &mut String, area: Rect, buf: &mut Buffer, cursor: &mut u16) {
    if text.trim().is_empty() {
        text.clear();
        return;
    }
    let lines = wrap_prefixed(text, area.width as usize, "", muted_style());
    render_lines(lines, area, buf, cursor);
    text.clear();
}

fn render_lines(lines: Vec<Line<'static>>, area: Rect, buf: &mut Buffer, cursor: &mut u16) {
    let rect = take_area(area, cursor, lines.len() as u16);
    if rect.is_empty() {
        return;
    }
    Paragraph::new(lines).render(rect, buf);
    *cursor = rect.bottom();
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

fn muted_style() -> Style {
    Style::default().fg(Color::Rgb(148, 163, 184))
}

fn short_id(id: &str) -> String {
    let count = id.chars().count();
    if count <= 12 {
        id.to_string()
    } else {
        let head = id.chars().take(6).collect::<String>();
        let tail = id
            .chars()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!("{head}…{tail}")
    }
}
