//! NostrKindRegistry for the TUI (F-CR-06).
//!
//! Single source of truth for kind → renderer dispatch in the terminal.

use std::collections::HashMap;
use std::sync::Arc;

use nmp_content::embed_projection::EmbedKindProjection;
use nmp_content::wire::{ContentTreeWire, WireNode};

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::kind_renderer::{KindRenderer, KindRendererRef};

/// The registry consulted by `EmbeddedEvent` (and by `NostrContentView`).
pub struct NostrKindRegistry {
    short_note: Option<KindRendererRef>,
    article: Option<KindRendererRef>,
    highlight: Option<KindRendererRef>,
    profile: Option<KindRendererRef>,
    unknown_by_kind: HashMap<u32, KindRendererRef>,
    fallback: KindRendererRef,
}

impl NostrKindRegistry {
    pub fn new(fallback: KindRendererRef) -> Self {
        Self {
            short_note: None,
            article: None,
            highlight: None,
            profile: None,
            unknown_by_kind: HashMap::new(),
            fallback,
        }
    }

    /// Installs the built-in default renderer for each known projection variant,
    /// plus `DefaultUnknownRenderer` as the fallback for unregistered numeric kinds.
    /// Replace any slot with `set_*` to swap in a richer handler (e.g. F-CR-09).
    pub fn make_default() -> Self {
        let mut reg = Self::new(Arc::new(DefaultUnknownRenderer));
        reg.short_note = Some(Arc::new(DefaultShortNoteRenderer));
        reg.article = Some(Arc::new(DefaultArticleRenderer));
        reg.highlight = Some(Arc::new(DefaultHighlightRenderer));
        reg.profile = Some(Arc::new(DefaultProfileRenderer));
        reg
    }

    pub fn set_short_note(&mut self, r: KindRendererRef) {
        self.short_note = Some(r);
    }

    pub fn set_article(&mut self, r: KindRendererRef) {
        self.article = Some(r);
    }

    pub fn set_highlight(&mut self, r: KindRendererRef) {
        self.highlight = Some(r);
    }

    pub fn set_profile(&mut self, r: KindRendererRef) {
        self.profile = Some(r);
    }

    pub fn register_unknown(&mut self, kind: u32, r: KindRendererRef) {
        self.unknown_by_kind.insert(kind, r);
    }

    pub fn resolve(&self, projection: &EmbedKindProjection) -> &dyn KindRenderer {
        match projection {
            EmbedKindProjection::ShortNote(_) => {
                self.short_note.as_deref().unwrap_or(self.fallback.as_ref())
            }
            EmbedKindProjection::Article(_) => {
                self.article.as_deref().unwrap_or(self.fallback.as_ref())
            }
            EmbedKindProjection::Highlight(_) => {
                self.highlight.as_deref().unwrap_or(self.fallback.as_ref())
            }
            EmbedKindProjection::Profile(_) => {
                self.profile.as_deref().unwrap_or(self.fallback.as_ref())
            }
            EmbedKindProjection::Unknown(p) => self
                .unknown_by_kind
                .get(&p.kind)
                .map(|r| r.as_ref())
                .unwrap_or(self.fallback.as_ref()),
        }
    }
}

/// Default renderer for `ShortNoteProjection` (kind:1 quoted notes).
pub struct DefaultShortNoteRenderer;

impl KindRenderer for DefaultShortNoteRenderer {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        _ctx: &nmp_content::context::RenderContext,
        _registry: &NostrKindRegistry,
        area: ratatui::layout::Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        let EmbedKindProjection::ShortNote(note) = projection else {
            return;
        };
        let author = note
            .author_display_name
            .clone()
            .unwrap_or_else(|| short_id(&note.author_pubkey));
        render_two_line(
            &format!("note · {author}"),
            &tree_text(&note.content_tree),
            area,
            buf,
        );
    }

    fn preferred_height(&self, projection: &EmbedKindProjection, width: u16) -> u16 {
        let EmbedKindProjection::ShortNote(note) = projection else {
            return 2;
        };
        text_height(&tree_text(&note.content_tree), width)
            .saturating_add(1)
            .max(2)
    }
}

/// Default renderer for `ArticleProjection` (kind:30023).
/// Shows title + summary. Replace via `registry.set_article(...)` for a richer card (F-CR-09).
pub struct DefaultArticleRenderer;

impl KindRenderer for DefaultArticleRenderer {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        _ctx: &nmp_content::context::RenderContext,
        _registry: &NostrKindRegistry,
        area: ratatui::layout::Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        let EmbedKindProjection::Article(article) = projection else {
            return;
        };
        let author = article
            .author_display_name
            .clone()
            .unwrap_or_else(|| short_id(&article.author_pubkey));
        let title = article.title.as_deref().unwrap_or("article");
        let summary = article
            .summary
            .clone()
            .unwrap_or_else(|| tree_text(&article.content_tree));
        render_two_line(&format!("{title} · {author}"), &summary, area, buf);
    }

    fn preferred_height(&self, projection: &EmbedKindProjection, width: u16) -> u16 {
        let EmbedKindProjection::Article(article) = projection else {
            return 2;
        };
        let summary = article
            .summary
            .clone()
            .unwrap_or_else(|| tree_text(&article.content_tree));
        text_height(&summary, width).saturating_add(1).max(2)
    }
}

/// Default renderer for `HighlightProjection` (kind:9802).
/// Shows highlighted text + source. Replace via `registry.set_highlight(...)` for F-CR-10.
pub struct DefaultHighlightRenderer;

impl KindRenderer for DefaultHighlightRenderer {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        _ctx: &nmp_content::context::RenderContext,
        _registry: &NostrKindRegistry,
        area: ratatui::layout::Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        let EmbedKindProjection::Highlight(highlight) = projection else {
            return;
        };
        let author = highlight
            .author_display_name
            .clone()
            .unwrap_or_else(|| short_id(&highlight.author_pubkey));
        render_two_line(
            &format!("highlight · {author}"),
            &highlight.highlighted_text,
            area,
            buf,
        );
    }

    fn preferred_height(&self, projection: &EmbedKindProjection, width: u16) -> u16 {
        let EmbedKindProjection::Highlight(highlight) = projection else {
            return 2;
        };
        text_height(&highlight.highlighted_text, width)
            .saturating_add(1)
            .max(2)
    }
}

/// Default renderer for `ProfileProjection` (kind:0).
/// Shows display name + about. Replace via `registry.set_profile(...)` for F-CR-11.
pub struct DefaultProfileRenderer;

impl KindRenderer for DefaultProfileRenderer {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        _ctx: &nmp_content::context::RenderContext,
        _registry: &NostrKindRegistry,
        area: ratatui::layout::Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        let EmbedKindProjection::Profile(profile) = projection else {
            return;
        };
        let label = profile
            .display_name
            .clone()
            .unwrap_or_else(|| short_id(&profile.pubkey));
        let about = profile.about.clone().unwrap_or_default();
        render_two_line("profile", &format!("{label} — {about}"), area, buf);
    }

    fn preferred_height(&self, projection: &EmbedKindProjection, width: u16) -> u16 {
        let EmbedKindProjection::Profile(profile) = projection else {
            return 2;
        };
        let about = profile.about.clone().unwrap_or_default();
        text_height(&about, width).saturating_add(1).max(2)
    }
}

/// Fallback renderer for `EmbedKindProjection::Unknown` — numeric Nostr kinds
/// that have no registered handler. Knows nothing about named variants.
pub struct DefaultUnknownRenderer;

impl KindRenderer for DefaultUnknownRenderer {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        _ctx: &nmp_content::context::RenderContext,
        _registry: &NostrKindRegistry,
        area: ratatui::layout::Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        let EmbedKindProjection::Unknown(unknown) = projection else {
            return;
        };
        let author = unknown
            .author_display_name
            .clone()
            .unwrap_or_else(|| short_id(&unknown.author_pubkey));
        let body = if unknown.content.is_empty() {
            tree_text(&unknown.content_tree)
        } else {
            unknown.content.clone()
        };
        render_two_line(&format!("kind:{} · {author}", unknown.kind), &body, area, buf);
    }

    fn preferred_height(&self, projection: &EmbedKindProjection, width: u16) -> u16 {
        let EmbedKindProjection::Unknown(unknown) = projection else {
            return 2;
        };
        let body = if unknown.content.is_empty() {
            tree_text(&unknown.content_tree)
        } else {
            unknown.content.clone()
        };
        text_height(&body, width).saturating_add(1).max(2)
    }
}

fn render_two_line(header: &str, body: &str, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
    let lines = vec![
        Line::from(Span::styled(
            header.to_string(),
            Style::default().fg(Color::Rgb(148, 163, 184)),
        )),
        Line::from(Span::raw(body.to_string())),
    ];
    Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .render(area, buf);
}

fn tree_text(tree: &ContentTreeWire) -> String {
    let mut out = Vec::new();
    for root in &tree.roots {
        if let Some(node) = tree.nodes.get(*root as usize) {
            let text = node_text(tree, node);
            if !text.is_empty() {
                out.push(text);
            }
        }
    }
    out.join("\n")
}

fn node_text(tree: &ContentTreeWire, node: &WireNode) -> String {
    match node {
        WireNode::Text { text } => text.clone(),
        WireNode::Mention { uri } => format!("@{}", short_id(&uri.primary_id)),
        WireNode::EventRef { uri } => format!("nostr:{}", short_id(&uri.primary_id)),
        WireNode::Hashtag { tag } => format!("#{tag}"),
        WireNode::Url { url } => url.clone(),
        WireNode::Media { urls, media_kind } => {
            format!("[{:?} media: {}]", media_kind, urls.len())
        }
        WireNode::Emoji { shortcode, .. } => format!(":{shortcode}:"),
        WireNode::Invoice { .. } => "[invoice]".to_string(),
        WireNode::Heading { children, .. }
        | WireNode::Paragraph { children }
        | WireNode::BlockQuote { children }
        | WireNode::Emphasis { children }
        | WireNode::Strong { children }
        | WireNode::Link { children, .. } => children_text(tree, children),
        WireNode::CodeBlock { body, .. } => body.clone(),
        WireNode::List {
            ordered_start,
            items,
        } => items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let marker = ordered_start
                    .map(|start| format!("{}.", start + idx as u64))
                    .unwrap_or_else(|| "-".to_string());
                format!("{marker} {}", children_text(tree, item))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        WireNode::InlineCode { code } => format!("`{code}`"),
        WireNode::Image { alt, src, .. } => src
            .as_deref()
            .map(|src| format!("[image: {alt} {src}]"))
            .unwrap_or_else(|| format!("[image: {alt}]")),
        WireNode::SoftBreak => " ".to_string(),
        WireNode::HardBreak => "\n".to_string(),
        WireNode::Rule => String::new(),
        WireNode::Placeholder { reason } => format!("[{reason:?}]"),
    }
}

fn children_text(tree: &ContentTreeWire, children: &[u32]) -> String {
    children
        .iter()
        .filter_map(|idx| tree.nodes.get(*idx as usize))
        .map(|node| node_text(tree, node))
        .collect::<Vec<_>>()
        .join("")
}

fn text_height(body: &str, width: u16) -> u16 {
    let width = width.max(1) as usize;
    body.lines()
        .map(|line| (line.chars().count().max(1).saturating_add(width - 1) / width) as u16)
        .sum::<u16>()
        .max(1)
}

fn short_id(hex: &str) -> String {
    if hex.chars().count() > 8 {
        hex.chars().take(8).collect()
    } else {
        hex.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nmp_content::{ContentTreeWire, UnknownProjection};
    use ratatui::{buffer::Buffer, layout::Rect};

    use super::*;

    struct HeightRenderer(u16);

    impl KindRenderer for HeightRenderer {
        fn render(
            &self,
            _projection: &EmbedKindProjection,
            _ctx: &nmp_content::RenderContext,
            _registry: &NostrKindRegistry,
            _area: Rect,
            _buf: &mut Buffer,
        ) {
        }

        fn preferred_height(&self, _projection: &EmbedKindProjection, _width: u16) -> u16 {
            self.0
        }
    }

    #[test]
    fn unknown_kind_specific_renderer_overrides_fallback() {
        let mut registry = NostrKindRegistry::make_default();
        registry.register_unknown(30_402, Arc::new(HeightRenderer(7)));

        let projection = unknown_projection(30_402);
        assert_eq!(
            registry
                .resolve(&projection)
                .preferred_height(&projection, 80),
            7
        );
    }

    #[test]
    fn unknown_kind_without_registration_uses_fallback() {
        let registry = NostrKindRegistry::make_default();
        let projection = unknown_projection(39_000);

        assert!(
            registry
                .resolve(&projection)
                .preferred_height(&projection, 80)
                >= 2
        );
    }

    fn unknown_projection(kind: u32) -> EmbedKindProjection {
        EmbedKindProjection::Unknown(UnknownProjection {
            kind,
            author_pubkey: "a".repeat(64),
            author_display_name: None,
            author_picture_url: None,
            created_at: 0,
            content: "hello".to_string(),
            content_tree: ContentTreeWire::default(),
            tags: Vec::new(),
            alt_text: None,
        })
    }
}
