use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use super::{
    content_render_data::ContentRenderData,
    content_tree_wire::{ContentTreeWire, WireNode},
};

/// Terminal quote/event reference card.
pub struct NostrQuoteCard<'a> {
    tree: &'a ContentTreeWire,
    node: &'a WireNode,
    render_data: Option<&'a ContentRenderData>,
    style: Style,
}

impl<'a> NostrQuoteCard<'a> {
    pub fn new(tree: &'a ContentTreeWire, node: &'a WireNode) -> Self {
        Self {
            tree,
            node,
            render_data: None,
            style: Style::default().fg(Color::Rgb(203, 213, 225)),
        }
    }

    pub fn render_data(mut self, render_data: Option<&'a ContentRenderData>) -> Self {
        self.render_data = render_data;
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
                    lines.extend(wrap_prefixed(&body, width, "  "));
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
                .flat_map(|line| wrap_prefixed(line, width, "> "))
                .collect(),
            _ => Vec::new(),
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

impl Widget for NostrQuoteCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(Color::Rgb(71, 85, 105)));
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(self.lines(inner.width as usize)).render(inner, buf);
    }
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
    let mut out = Vec::new();
    let mut line = String::new();
    for word in value.split_whitespace() {
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
