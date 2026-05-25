use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use super::{
    content_render_data::ContentRenderData,
    content_tree_wire::{ContentTreeWire, WireNode},
    nostr_mention_chip::NostrMentionChip,
};

/// Minimal inline renderer for text, mentions, hashtags, URLs, and breaks.
pub struct NostrMinimalContent<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
}

impl<'a> NostrMinimalContent<'a> {
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
        wrap_spans(self.spans(), width)
    }

    fn spans(&self) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for root in &self.tree.roots {
            append_inline(self.tree, self.render_data, *root, &mut spans);
        }
        spans
    }
}

impl Widget for NostrMinimalContent<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.lines(area.width as usize))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

fn append_inline(
    tree: &ContentTreeWire,
    render_data: Option<&ContentRenderData>,
    index: usize,
    spans: &mut Vec<Span<'static>>,
) {
    let Some(node) = tree.node(index) else {
        return;
    };
    match node {
        WireNode::Text(text) => spans.push(Span::raw(text.clone())),
        WireNode::Mention(uri) => spans.push(
            NostrMentionChip::new(uri)
                .profile(render_data.and_then(|data| data.profile_for(uri)))
                .span(),
        ),
        WireNode::Hashtag(tag) => spans.push(Span::styled(
            format!("#{tag}"),
            Style::default().fg(Color::Rgb(45, 212, 191)),
        )),
        WireNode::Url(url) => spans.push(Span::styled(
            url.clone(),
            Style::default().fg(Color::Rgb(96, 165, 250)),
        )),
        WireNode::InlineCode(code) => spans.push(Span::styled(
            format!("`{code}`"),
            Style::default().fg(Color::Rgb(253, 186, 116)),
        )),
        WireNode::EventRef(uri) => spans.push(Span::styled(
            format!("nostr:{}", short_id(&uri.primary_id)),
            Style::default().fg(Color::Rgb(196, 181, 253)),
        )),
        WireNode::Emoji { shortcode, .. } => spans.push(Span::raw(format!(":{shortcode}:"))),
        WireNode::Invoice { invoice } => spans.push(Span::styled(
            format!("[{} invoice]", invoice.kind.to_ascii_lowercase()),
            Style::default().fg(Color::Rgb(250, 204, 21)),
        )),
        WireNode::SoftBreak => spans.push(Span::raw(" ")),
        WireNode::HardBreak => spans.push(Span::raw("\n")),
        WireNode::Paragraph { children }
        | WireNode::Emphasis { children }
        | WireNode::Strong { children }
        | WireNode::Link { children, .. } => {
            for child in children {
                append_inline(tree, render_data, *child, spans);
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
        _ => {
            let label = node.inline_label(tree);
            if !label.is_empty() {
                spans.push(Span::raw(label));
            }
        }
    }
}

fn wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }
    let text = spans
        .into_iter()
        .map(|span| span.content.to_string())
        .collect::<String>();
    wrap_words(&text.replace('\n', " "), width)
        .into_iter()
        .map(Line::from)
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
