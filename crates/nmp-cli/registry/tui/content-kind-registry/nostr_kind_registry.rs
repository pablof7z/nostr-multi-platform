//! NostrKindRegistry for the TUI (F-CR-06).
//!
//! Single source of truth for kind → renderer dispatch in the terminal.

use std::collections::HashMap;
use std::sync::Arc;

use nmp_content::embed_projection::EmbedKindProjection;
use nmp_content::wire::{ContentTreeWire, WireNode};

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget, Wrap};

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
/// Renders in a rounded box matching `DefaultArticleRenderer`, with author
/// byline and relative timestamp.
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
        if area.height < 4 || area.width < 6 {
            return;
        }

        let author = note
            .author_display_name
            .clone()
            .unwrap_or_else(|| short_id(&note.author_pubkey));
        let body = tree_text(&note.content_tree);
        let rel_time = format_relative_time(note.created_at);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(71, 85, 105)));
        let inner = block.inner(area);
        block.render(area, buf);

        let content = Rect {
            x: inner.x + 1,
            y: inner.y,
            width: inner.width.saturating_sub(1),
            height: inner.height,
        };
        if content.width == 0 || content.height == 0 {
            return;
        }

        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(content);

        // Byline: ● author · relative_time
        Paragraph::new(Line::from(vec![
            Span::styled("\u{25CF} ", Style::default().fg(Color::Rgb(220, 38, 38))),
            Span::styled(author, Style::default().fg(Color::Rgb(203, 213, 225))),
            Span::styled(
                format!(" \u{00B7} {}", rel_time),
                Style::default().fg(Color::Rgb(100, 116, 139)),
            ),
        ]))
        .render(rows[0], buf);

        // Body
        Paragraph::new(Line::from(Span::styled(
            body,
            Style::default().fg(Color::Rgb(148, 163, 184)),
        )))
        .wrap(Wrap { trim: true })
        .render(rows[1], buf);
    }

    fn preferred_height(&self, projection: &EmbedKindProjection, width: u16) -> u16 {
        let EmbedKindProjection::ShortNote(note) = projection else {
            return 4;
        };
        let wrap_width = width.saturating_sub(3).max(1);
        text_height(&tree_text(&note.content_tree), wrap_width)
            .saturating_add(1) // byline
            .saturating_add(2) // top + bottom borders
            .max(4)
    }
}

/// Default renderer for `ArticleProjection` (kind:30023).
/// Continuous-byline card: rounded box, bold title, `● author · date · N min read`, summary.
pub struct DefaultArticleRenderer;

impl KindRenderer for DefaultArticleRenderer {
    fn render(
        &self,
        projection: &EmbedKindProjection,
        _ctx: &nmp_content::context::RenderContext,
        _registry: &NostrKindRegistry,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        let EmbedKindProjection::Article(article) = projection else {
            return;
        };
        if area.height < 5 || area.width < 6 {
            return;
        }

        let author = article
            .author_display_name
            .as_deref()
            .unwrap_or_else(|| &article.author_pubkey[..8.min(article.author_pubkey.len())]);
        let title = article.title.as_deref().unwrap_or("article");
        let summary = article
            .summary
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| tree_text(&article.content_tree));
        let short_date = format_short_date(article.created_at);
        let reading_min = estimate_reading_time(title, &summary);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(71, 85, 105)));
        let inner = block.inner(area);
        block.render(area, buf);

        let content = Rect {
            x: inner.x + 1,
            y: inner.y,
            width: inner.width.saturating_sub(1),
            height: inner.height,
        };
        if content.width == 0 || content.height == 0 {
            return;
        }

        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(content);

        // Title
        let title_str = truncate_chars(title, content.width as usize);
        Paragraph::new(Line::from(Span::styled(
            title_str,
            Style::default().fg(Color::Rgb(241, 245, 249)).add_modifier(Modifier::BOLD),
        )))
        .render(rows[0], buf);

        // Byline: ● Author · Date · N min read
        let meta = format!(" \u{00B7} {} \u{00B7} {} min read", short_date, reading_min);
        Paragraph::new(Line::from(vec![
            Span::styled("\u{25CF} ", Style::default().fg(Color::Rgb(220, 38, 38))),
            Span::styled(author.to_string(), Style::default().fg(Color::Rgb(203, 213, 225))),
            Span::styled(meta, Style::default().fg(Color::Rgb(100, 116, 139))),
        ]))
        .render(rows[1], buf);

        // Summary
        let summary_str = truncate_chars(&summary, content.width as usize);
        Paragraph::new(Line::from(Span::styled(
            summary_str,
            Style::default().fg(Color::Rgb(148, 163, 184)),
        )))
        .render(rows[2], buf);
    }

    fn preferred_height(&self, _projection: &EmbedKindProjection, _width: u16) -> u16 {
        5
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

fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let mut out: String = chars.iter().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

fn format_relative_time(unix_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let delta = now.saturating_sub(unix_secs);

    if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86400 {
        format!("{}h ago", delta / 3600)
    } else if delta < 30 * 86400 {
        format!("{}d ago", delta / 86400)
    } else {
        format_short_date(unix_secs)
    }
}

fn format_short_date(unix_secs: u64) -> String {
    // Days since Unix epoch → calendar date (Gregorian, no external crate).
    let days = unix_secs / 86400;
    let mut y = 1970u32;
    let mut d = days as u32;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if d < days_in_year { break; }
        d -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31u32, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let month_names = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    let mut month = 0usize;
    while month < 12 && d >= month_days[month] {
        d -= month_days[month];
        month += 1;
    }
    format!("{} {}", month_names[month.min(11)], d + 1)
}

fn estimate_reading_time(title: &str, summary: &str) -> u32 {
    let words = title.split_whitespace().count() + summary.split_whitespace().count();
    // Assume full article is ~10× the summary word count; 200 wpm average.
    let estimated_words = (words * 10).max(200);
    ((estimated_words as f32 / 200.0).ceil() as u32).max(1)
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
