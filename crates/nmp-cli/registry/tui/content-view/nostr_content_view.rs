use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use super::{
    content_render_data::ContentRenderData,
    content_tree_wire::{ContentTreeWire, WireNode},
    nostr_media_grid::NostrMediaGrid,
    nostr_mention_chip::NostrMentionChip,
    nostr_quote_card::NostrQuoteCard,
};

/// Full terminal renderer for the Rust-owned `ContentTreeWire` projection.
pub struct NostrContentView<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
}

impl<'a> NostrContentView<'a> {
    pub fn new(tree: &'a ContentTreeWire) -> Self {
        Self {
            tree,
            render_data: None,
        }
    }

    pub fn render_data(mut self, render_data: Option<&'a ContentRenderData>) -> Self {
        self.render_data = render_data;
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
                    lines.extend(wrap_prefixed(&body, width, &format!("{marker} ")));
                }
            }
            WireNode::Rule => lines.push(Line::from("─".repeat(width.min(48)))),
            WireNode::Media { urls, kind } => {
                lines.extend(NostrMediaGrid::new(urls, kind).lines(width));
            }
            WireNode::Image { alt, src, .. } => {
                let target = src.as_deref().unwrap_or("missing src");
                lines.extend(wrap_prefixed(&format!("{alt} {target}"), width, "[image] "));
            }
            _ => {
                let text = node.inline_label(self.tree);
                if !text.is_empty() {
                    lines.extend(wrap_plain(&text, width));
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
            WireNode::EventRef(uri) => spans.push(Span::styled(
                format!("nostr:{}", short_id(&uri.primary_id)),
                Style::default().fg(Color::Rgb(196, 181, 253)),
            )),
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
        Paragraph::new(self.lines(area.width as usize))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

fn wrap_plain(value: &str, width: usize) -> Vec<Line<'static>> {
    wrap_words(value, width)
        .into_iter()
        .map(Line::from)
        .collect()
}

fn wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut used = 0usize;

    for span in spans {
        let style = span.style;
        let text = span.content.to_string();
        let mut word = String::new();
        for ch in text.chars() {
            if ch == '\n' {
                push_piece(&mut lines, &mut current, &mut used, &word, style, width);
                word.clear();
                lines.push(line_from_spans(std::mem::take(&mut current)));
                used = 0;
            } else if ch.is_whitespace() {
                push_piece(&mut lines, &mut current, &mut used, &word, style, width);
                word.clear();
                if used > 0 && used < width {
                    current.push(Span::styled(" ".to_string(), style));
                    used += 1;
                }
            } else {
                word.push(ch);
            }
        }
        push_piece(&mut lines, &mut current, &mut used, &word, style, width);
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(line_from_spans(current));
    }
    lines
}

fn push_piece(
    lines: &mut Vec<Line<'static>>,
    current: &mut Vec<Span<'static>>,
    used: &mut usize,
    piece: &str,
    style: Style,
    width: usize,
) {
    if piece.is_empty() {
        return;
    }
    for chunk in split_chars(piece, width) {
        let len = chunk.chars().count();
        if *used > 0 && *used + len > width {
            lines.push(line_from_spans(std::mem::take(current)));
            *used = 0;
        }
        current.push(Span::styled(chunk, style));
        *used += len;
    }
}

fn split_chars(value: &str, width: usize) -> Vec<String> {
    if value.chars().count() <= width {
        return vec![value.to_string()];
    }
    let mut out = Vec::new();
    let mut chunk = String::new();
    for ch in value.chars() {
        if chunk.chars().count() == width {
            out.push(std::mem::take(&mut chunk));
        }
        chunk.push(ch);
    }
    if !chunk.is_empty() {
        out.push(chunk);
    }
    out
}

fn line_from_spans(spans: Vec<Span<'static>>) -> Line<'static> {
    let mut out = Vec::new();
    for span in spans {
        if span.content == "\n" {
            continue;
        }
        out.push(span);
    }
    Line::from(out)
}

fn wrap_prefixed(value: &str, width: usize, prefix: &str) -> Vec<Line<'static>> {
    let body_width = width.saturating_sub(prefix.chars().count()).max(1);
    wrap_words(value, body_width)
        .into_iter()
        .map(|line| {
            Line::from(vec![
                Span::styled(
                    prefix.to_string(),
                    Style::default().fg(Color::Rgb(148, 163, 184)),
                ),
                Span::raw(line),
            ])
        })
        .collect()
}

fn wrap_words(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut line = String::new();
    for word in value.replace('\n', " ").split_whitespace() {
        let next = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if next.chars().count() > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            line.push_str(word);
        } else {
            line = next;
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
    out
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
