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
    nostr_mention_chip::{NostrMentionChip, NostrMentionProfileHost},
    ratatui_text_wrap::wrap_spans,
};

/// Minimal inline renderer for text, mentions, hashtags, URLs, and breaks.
pub struct NostrMinimalContent<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
    profile_host: Option<&'a dyn NostrMentionProfileHost>,
    consumer_id: Option<&'a str>,
}

impl<'a> NostrMinimalContent<'a> {
    pub fn new(tree: &'a ContentTreeWire) -> Self {
        Self {
            tree,
            render_data: None,
            profile_host: None,
            consumer_id: None,
        }
    }

    pub fn render_data(mut self, render_data: Option<&'a ContentRenderData>) -> Self {
        self.render_data = render_data;
        self
    }

    pub fn profile_host(mut self, host: Option<&'a dyn NostrMentionProfileHost>) -> Self {
        self.profile_host = host;
        self
    }

    pub fn consumer_id(mut self, id: Option<&'a str>) -> Self {
        self.consumer_id = id;
        self
    }

    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        wrap_spans(self.spans(), width)
    }

    fn spans(&self) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for root in &self.tree.roots {
            append_inline(
                self.tree,
                self.render_data,
                self.profile_host,
                self.consumer_id,
                *root,
                &mut spans,
            );
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
    profile_host: Option<&dyn NostrMentionProfileHost>,
    consumer_id: Option<&str>,
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
                .profile_host(profile_host)
                .consumer_id(consumer_id)
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
        WireNode::EventRef(uri) => {
            let label = render_data
                .and_then(|data| data.event_for(uri))
                .map(|event| {
                    let body = if event.content_preview.is_empty() {
                        short_id(&event.id)
                    } else {
                        event.content_preview.clone()
                    };
                    format!("quote {}: {body}", event.author_label())
                })
                .unwrap_or_else(|| format!("quote {}", short_id(&uri.primary_id)));
            spans.push(Span::styled(
                label,
                Style::default().fg(Color::Rgb(196, 181, 253)),
            ));
        }
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
                append_inline(tree, render_data, profile_host, consumer_id, *child, spans);
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
