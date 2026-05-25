use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};
use ratatui_image::protocol::Protocol;

use super::{
    content_render_data::ContentRenderData,
    content_tree_wire::{ContentTreeWire, WireNode},
    nostr_media_grid::NostrMediaGrid,
    nostr_mention_chip::NostrMentionChip,
    nostr_quote_card::NostrQuoteCard,
    ratatui_text_wrap::{wrap_plain, wrap_prefixed, wrap_spans},
};

/// Full terminal renderer for the Rust-owned `ContentTreeWire` projection.
pub struct NostrContentView<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
    media_images: &'a [(&'a str, &'a Protocol)],
}

impl<'a> NostrContentView<'a> {
    pub fn new(tree: &'a ContentTreeWire) -> Self {
        Self {
            tree,
            render_data: None,
            media_images: &[],
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
        let mut lines = Vec::new();
        for root in &self.tree.roots {
            self.append_node(*root, width, &mut lines);
        }
        if lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines
    }

    fn append_node(&self, index: usize, width: usize, lines: &mut Vec<Line<'static>>) {
        let Some(node) = self.tree.node(index) else {
            return;
        };
        match node {
            WireNode::Paragraph { children } => {
                self.append_paragraph(children, width, lines);
            }
            WireNode::Heading { level, children } => {
                let prefix = "#".repeat((*level).clamp(1, 6) as usize);
                let mut spans = vec![Span::styled(
                    format!("{prefix} "),
                    Style::default().fg(Color::Rgb(248, 250, 252)),
                )];
                spans.extend(self.inline_spans(children));
                lines.extend(wrap_spans(spans, width));
            }
            WireNode::BlockQuote { .. } | WireNode::EventRef(_) => {
                lines.extend(
                    NostrQuoteCard::new(self.tree, node)
                        .render_data(self.render_data)
                        .media_images(self.media_images)
                        .lines(width),
                );
            }
            WireNode::CodeBlock { info, body } => {
                let title = info.as_deref().unwrap_or("code");
                lines.push(Line::from(Span::styled(
                    format!("```{title}"),
                    Style::default().fg(Color::Rgb(148, 163, 184)),
                )));
                lines.extend(body.lines().map(|line| {
                    Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::Rgb(253, 186, 116)),
                    ))
                }));
                lines.push(Line::from(Span::styled(
                    "```",
                    Style::default().fg(Color::Rgb(148, 163, 184)),
                )));
            }
            WireNode::List {
                ordered_start,
                items,
            } => {
                for (idx, item) in items.iter().enumerate() {
                    let marker = ordered_start
                        .map(|start| format!("{}.", start + idx as u64))
                        .unwrap_or_else(|| "-".to_string());
                    let body = self.text_for_nodes(item);
                    lines.extend(wrap_prefixed(
                        &body,
                        width,
                        &format!("{marker} "),
                        muted_style(),
                    ));
                }
            }
            WireNode::Rule => lines.push(Line::from("─".repeat(width.min(48)))),
            WireNode::Media { urls, kind } => {
                lines.extend(NostrMediaGrid::new(urls, kind).lines(width));
            }
            WireNode::Image { alt, src, .. } => {
                let target = src.as_deref().unwrap_or("missing src");
                lines.extend(wrap_prefixed(
                    &format!("{alt} {target}"),
                    width,
                    "[image] ",
                    muted_style(),
                ));
            }
            _ => {
                let mut spans = Vec::new();
                self.append_inline_node(index, &mut spans);
                if spans.is_empty() {
                    let text = node.inline_label(self.tree);
                    if !text.is_empty() {
                        lines.extend(wrap_plain(&text, width));
                    }
                } else {
                    lines.extend(wrap_spans(spans, width));
                }
            }
        }
    }

    fn append_paragraph(&self, children: &[usize], width: usize, lines: &mut Vec<Line<'static>>) {
        let mut inline = Vec::new();
        for child in children {
            let Some(node) = self.tree.node(*child) else {
                continue;
            };
            if matches!(node, WireNode::EventRef(_)) {
                if !inline.is_empty() {
                    lines.extend(wrap_spans(std::mem::take(&mut inline), width));
                }
                lines.extend(
                    NostrQuoteCard::new(self.tree, node)
                        .render_data(self.render_data)
                        .media_images(self.media_images)
                        .lines(width),
                );
            } else {
                self.append_inline_node(*child, &mut inline);
            }
        }
        if !inline.is_empty() {
            lines.extend(wrap_spans(inline, width));
        }
    }

    fn inline_spans(&self, children: &[usize]) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for child in children {
            self.append_inline_node(*child, &mut spans);
        }
        spans
    }

    fn append_inline_node(&self, index: usize, spans: &mut Vec<Span<'static>>) {
        let Some(node) = self.tree.node(index) else {
            return;
        };
        match node {
            WireNode::Text(text) => spans.push(Span::raw(text.clone())),
            WireNode::Mention(uri) => spans.push(
                NostrMentionChip::new(uri)
                    .profile(self.render_data.and_then(|data| data.profile_for(uri)))
                    .span(),
            ),
            WireNode::EventRef(uri) => {
                let label = self
                    .render_data
                    .and_then(|data| data.event_for(uri))
                    .map(|event| format!("quote {}", event.author_label()))
                    .unwrap_or_else(|| format!("quote {}", short_id(&uri.primary_id)));
                spans.push(Span::styled(
                    label,
                    Style::default().fg(Color::Rgb(196, 181, 253)),
                ));
            }
            WireNode::Hashtag(tag) => spans.push(Span::styled(
                format!("#{tag}"),
                Style::default().fg(Color::Rgb(45, 212, 191)),
            )),
            WireNode::Url(url) => spans.push(Span::styled(
                url.clone(),
                Style::default().fg(Color::Rgb(96, 165, 250)),
            )),
            WireNode::Emoji { shortcode, .. } => spans.push(Span::raw(format!(":{shortcode}:"))),
            WireNode::Invoice { invoice } => spans.push(Span::styled(
                format!("[{} invoice]", invoice.kind.to_ascii_lowercase()),
                Style::default().fg(Color::Rgb(250, 204, 21)),
            )),
            WireNode::InlineCode(code) => spans.push(Span::styled(
                format!("`{code}`"),
                Style::default().fg(Color::Rgb(253, 186, 116)),
            )),
            WireNode::Emphasis { children } => {
                let start = spans.len();
                for child in children {
                    self.append_inline_node(*child, spans);
                }
                for span in &mut spans[start..] {
                    span.style = span.style.add_modifier(Modifier::ITALIC);
                }
            }
            WireNode::Strong { children } => {
                let start = spans.len();
                for child in children {
                    self.append_inline_node(*child, spans);
                }
                for span in &mut spans[start..] {
                    span.style = span.style.add_modifier(Modifier::BOLD);
                }
            }
            WireNode::Link { children, href } => {
                let start = spans.len();
                if children.is_empty() {
                    spans.push(Span::styled(
                        href.clone().unwrap_or_default(),
                        Style::default().fg(Color::Rgb(96, 165, 250)),
                    ));
                } else {
                    for child in children {
                        self.append_inline_node(*child, spans);
                    }
                    for span in &mut spans[start..] {
                        span.style = span.style.fg(Color::Rgb(96, 165, 250));
                    }
                }
            }
            WireNode::Image { alt, src, .. } => {
                let target = src.as_deref().unwrap_or("missing src");
                spans.push(Span::styled(
                    format!("[image: {alt} {target}]"),
                    Style::default().fg(Color::Rgb(186, 230, 253)),
                ));
            }
            WireNode::Media { urls, kind } => spans.push(Span::styled(
                format!("[{} media: {}]", kind.to_ascii_lowercase(), urls.len()),
                Style::default().fg(Color::Rgb(186, 230, 253)),
            )),
            WireNode::SoftBreak => spans.push(Span::raw(" ")),
            WireNode::HardBreak => spans.push(Span::raw("\n")),
            WireNode::Paragraph { children }
            | WireNode::Heading { children, .. }
            | WireNode::BlockQuote { children } => {
                for child in children {
                    self.append_inline_node(*child, spans);
                }
            }
            _ => {
                let text = node.inline_label(self.tree);
                if !text.is_empty() {
                    spans.push(Span::raw(text));
                }
            }
        }
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

impl Widget for NostrContentView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut cursor = area.y;
        for root in &self.tree.roots {
            self.render_node(*root, area, buf, &mut cursor);
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
            WireNode::EventRef(_) | WireNode::BlockQuote { .. } => {
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
                WireNode::EventRef(_) | WireNode::Media { .. } | WireNode::Image { .. } => {
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

fn muted_style() -> Style {
    Style::default().fg(Color::Rgb(148, 163, 184))
}
