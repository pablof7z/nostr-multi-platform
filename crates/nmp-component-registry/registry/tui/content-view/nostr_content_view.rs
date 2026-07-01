use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use nmp_content::embed_projection::EmbeddedEventEnvelope;
use nmp_content::EventRefResolver;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use ratatui_image::protocol::Protocol;

mod nostr_content_widget;

use super::{
    content_kind_registry::NostrKindRegistry,
    content_render_data::ContentRenderData,
    content_tree_wire::{ContentTreeWire, WireNode, WireUri},
    nostr_media_grid::NostrMediaGrid,
    nostr_mention_chip::{NostrMentionChip, NostrMentionProfileHost},
    ratatui_text_wrap::{wrap_plain, wrap_prefixed, wrap_spans},
};

pub struct NostrContentView<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
    media_images: &'a [(&'a str, &'a Protocol)],
    kind_registry: Option<&'a NostrKindRegistry>,
    embedded_events: Option<&'a BTreeMap<String, EmbeddedEventEnvelope>>,
    event_ref_resolver: Option<&'a dyn EventRefResolver>,
    profile_host: Option<&'a dyn NostrMentionProfileHost>,
    consumer_id: Option<&'a str>,
    // Per-render-pass seen-set. `Widget::render` consumes `self`, so the widget
    // is built fresh by the builder each frame — this set is naturally scoped
    // to a single render pass and dedups multiple references to the same URI
    // within one frame (D8: edge-triggered by render, no polling).
    claimed_this_frame: RefCell<HashSet<String>>,
}

impl<'a> NostrContentView<'a> {
    pub fn new(tree: &'a ContentTreeWire) -> Self {
        Self {
            tree,
            render_data: None,
            media_images: &[],
            kind_registry: None,
            embedded_events: None,
            event_ref_resolver: None,
            profile_host: None,
            consumer_id: None,
            claimed_this_frame: RefCell::new(HashSet::new()),
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

    pub fn kind_registry(mut self, registry: Option<&'a NostrKindRegistry>) -> Self {
        self.kind_registry = registry;
        self
    }

    pub fn embedded_events(
        mut self,
        events: Option<&'a BTreeMap<String, EmbeddedEventEnvelope>>,
    ) -> Self {
        self.embedded_events = events;
        self
    }

    /// Optional host-side sink used to initiate an upstream fetch the first
    /// time a `nostr:` event URI is encountered in a render pass (ADR-0034 /
    /// M16). Defaults to `None`, which preserves preview-only behaviour: no
    /// event refs are resolved and only pre-populated `embedded_events` resolve.
    pub fn event_ref_resolver(mut self, sink: Option<&'a dyn EventRefResolver>) -> Self {
        self.event_ref_resolver = sink;
        self
    }

    /// Optional host-side bridge used to claim and read profile projections for
    /// inline `nostr:npub` mentions. Defaults to `None`, which preserves
    /// preview-only behaviour and renders the reference fallback.
    pub fn profile_host(mut self, host: Option<&'a dyn NostrMentionProfileHost>) -> Self {
        self.profile_host = host;
        self
    }

    /// Optional consumer identifier passed alongside each `resolve_event_ref` call so the
    /// kernel can refcount per-host. Defaults to `None`; if either this or
    /// `event_ref_resolver` is unset, the resolve path is skipped entirely.
    pub fn consumer_id(mut self, id: Option<&'a str>) -> Self {
        self.consumer_id = id;
        self
    }

    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut root_pos = 0usize;
        while root_pos < self.tree.roots.len() {
            let root = self.tree.roots[root_pos];
            let Some(node) = self.tree.node(root) else {
                root_pos += 1;
                continue;
            };
            if is_inline_root(node) {
                let inline = self.collect_inline_roots(&mut root_pos);
                if !inline.is_empty() {
                    lines.extend(wrap_spans(inline, width));
                }
            } else {
                self.append_node(root, width, &mut lines);
                root_pos += 1;
            }
        }
        if lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines
    }

    fn collect_inline_roots(&self, root_pos: &mut usize) -> Vec<Span<'static>> {
        let mut inline = Vec::new();
        while *root_pos < self.tree.roots.len() {
            let root = self.tree.roots[*root_pos];
            let Some(node) = self.tree.node(root) else {
                *root_pos += 1;
                continue;
            };
            if !is_inline_root(node) {
                break;
            }
            self.append_inline_node(root, &mut inline);
            *root_pos += 1;
        }
        inline
    }

    pub fn preferred_height(&self, width: usize) -> u16 {
        self.tree
            .roots
            .iter()
            .map(|root| self.node_height(*root, width))
            .sum::<u16>()
            .max(1)
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
            WireNode::BlockQuote { children } => {
                lines.extend(self.blockquote_lines(children, width));
            }
            WireNode::EventRef(uri) => {
                lines.extend(self.event_ref_lines(uri, width));
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
                lines.extend(
                    NostrMediaGrid::new(urls, kind)
                        .images(self.media_images)
                        .lines(width),
                );
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
            if let WireNode::EventRef(uri) = node {
                if !inline.is_empty() {
                    lines.extend(wrap_spans(std::mem::take(&mut inline), width));
                }
                lines.extend(self.event_ref_lines(uri, width));
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
                    .profile_host(self.profile_host)
                    .consumer_id(self.consumer_id)
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

    /// Inline fallback lines for a markdown blockquote (`> …`). Used by the
    /// preview/`lines()` path; the interactive widget path renders blockquotes
    /// with the same `> ` prefix in `nostr_content_widget.rs`.
    fn blockquote_lines(&self, children: &[usize], width: usize) -> Vec<Line<'static>> {
        self.text_for_nodes(children)
            .split('\n')
            .flat_map(|line| wrap_prefixed(line, width, "> ", muted_style()))
            .collect()
    }

    /// Inline fallback lines for an unresolved `nostr:` event reference. The
    /// interactive widget path renders the resolved event via the kind-registry
    /// `EmbeddedEvent`; this preview/`lines()` path has no host, so it surfaces
    /// the same one-line `quote <id>` affordance the legacy quote card used.
    fn event_ref_lines(&self, uri: &WireUri, width: usize) -> Vec<Line<'static>> {
        let label = self
            .render_data
            .and_then(|data| data.event_for(uri))
            .map(|event| format!("quote {} · kind:{}", event.author_label(), event.kind))
            .unwrap_or_else(|| format!("quote {}", short_id(&uri.primary_id)));
        wrap_prefixed(&label, width, "", muted_style())
    }

    fn node_height(&self, index: usize, width: usize) -> u16 {
        let Some(node) = self.tree.node(index) else {
            return 0;
        };
        match node {
            WireNode::Paragraph { children } => self.paragraph_height(children, width),
            WireNode::Media { urls, kind } => NostrMediaGrid::new(urls, kind)
                .images(self.media_images)
                .preferred_height()
                .saturating_add(1),
            WireNode::Image { src: Some(src), .. } => {
                NostrMediaGrid::new(std::slice::from_ref(src), "image")
                    .images(self.media_images)
                    .preferred_height()
                    .saturating_add(1)
            }
            WireNode::EventRef(uri) => self.event_ref_lines(uri, width).len().max(1) as u16,
            WireNode::BlockQuote { children } => {
                self.blockquote_lines(children, width).len().max(1) as u16
            }
            _ => {
                let mut lines = Vec::new();
                self.append_node(index, width, &mut lines);
                lines.len().max(1) as u16
            }
        }
    }

    fn paragraph_height(&self, children: &[usize], width: usize) -> u16 {
        let mut inline = Vec::new();
        let mut height = 0u16;
        for child in children {
            match self.tree.node(*child) {
                Some(WireNode::EventRef(_) | WireNode::Media { .. } | WireNode::Image { .. }) => {
                    height = height.saturating_add(nonempty_wrapped_height(
                        std::mem::take(&mut inline),
                        width,
                    ));
                    height = height.saturating_add(self.node_height(*child, width));
                }
                Some(_) => self.append_inline_node(*child, &mut inline),
                None => {}
            }
        }
        height.saturating_add(nonempty_wrapped_height(inline, width))
    }
}

fn nonempty_wrapped_height(spans: Vec<Span<'static>>, width: usize) -> u16 {
    wrap_spans(spans, width)
        .into_iter()
        .filter(|line| line.spans.iter().any(|span| !span.content.is_empty()))
        .count() as u16
}

#[rustfmt::skip]
fn is_inline_root(node: &WireNode) -> bool {
    use WireNode::*;
    !matches!(node, Paragraph { .. } | Heading { .. } | BlockQuote { .. } | CodeBlock { .. } | List { .. } | Rule | Media { .. } | Image { .. } | EventRef(_))
}

fn short_id(id: &str) -> String {
    if id.len() > 12 {
        return format!("{}…{}", &id[..6], &id[id.len() - 6..]);
    }
    id.to_string()
}

fn muted_style() -> Style {
    Style::default().fg(Color::Rgb(148, 163, 184))
}
