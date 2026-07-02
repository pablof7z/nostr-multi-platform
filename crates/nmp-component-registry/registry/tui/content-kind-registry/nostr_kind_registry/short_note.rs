//! Default renderer for kind:1 — quoted short text notes (NIP-01), the
//! `ShortNoteProjection` variant of `EmbedKindProjection`.

use nmp_content::embed_projection::EmbedKindProjection;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget, Wrap};

use super::super::super::nostr_mention_chip::NostrMentionProfileHost;
use super::super::kind_renderer::{author_byline, KindRenderer};
use super::text_layout::{format_relative_time, text_height, tree_text};
use super::NostrKindRegistry;

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
        host: Option<&dyn NostrMentionProfileHost>,
        consumer_id: Option<&str>,
        area: ratatui::layout::Rect,
        buf: &mut ratatui::buffer::Buffer,
    ) {
        let EmbedKindProjection::ShortNote(note) = projection else {
            return;
        };
        if area.height < 4 || area.width < 6 {
            return;
        }

        // Component-owned kind:0: this byline claims the author's profile and
        // reads the live-resolved name, instead of painting the static
        // `author_display_name` projection field (mirrors iOS PR #833).
        let author = author_byline(host, consumer_id, &note.author_pubkey);
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use nmp_content::{ContentTreeWire, ShortNoteProjection};
    use nmp_core::display::short_npub;
    use ratatui::buffer::Buffer;

    use super::super::super::super::content_render_data::ContentProfileRenderData;
    use super::*;

    const SHOWCASE_PUBKEY: &str =
        "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";

    /// Fake host that records claims and returns a known live-resolved name —
    /// the TUI analogue of `mention_label_claims_and_reads_host_projection`.
    struct FakeAuthorHost {
        display: Option<String>,
        claimed: RefCell<Vec<(String, String)>>,
    }

    impl NostrMentionProfileHost for FakeAuthorHost {
        fn resolve_ref(&self, pubkey: &str, consumer_id: &str) {
            self.claimed
                .borrow_mut()
                .push((pubkey.to_string(), consumer_id.to_string()));
        }

        fn profile_for_pubkey(&self, pubkey: &str) -> Option<ContentProfileRenderData> {
            Some(ContentProfileRenderData {
                pubkey: pubkey.to_string(),
                display_name: self.display.clone(),
                npub: None,
                picture_url: None,
            })
        }
    }

    fn buffer_text(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if let Some(cell) = buf.cell((x, y)) {
                    out.push_str(cell.symbol());
                }
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn author_byline_claims_and_reads_live_name() {
        let host = FakeAuthorHost {
            display: Some("pablof7z".to_string()),
            claimed: RefCell::new(Vec::new()),
        };

        let byline = author_byline(Some(&host), Some("content-kind-registry"), SHOWCASE_PUBKEY);

        assert_eq!(byline, "pablof7z");
        assert_eq!(
            host.claimed.borrow().as_slice(),
            [(
                SHOWCASE_PUBKEY.to_string(),
                "content-kind-registry".to_string()
            )]
        );
    }

    #[test]
    fn author_byline_falls_back_to_npub_short_not_hex() {
        // Host wired but profile unresolved: must render the Rust-formatted
        // npub_short, never raw hex and never an 8-char hex prefix.
        let host = FakeAuthorHost {
            display: None,
            claimed: RefCell::new(Vec::new()),
        };

        let byline = author_byline(Some(&host), Some("content-kind-registry"), SHOWCASE_PUBKEY);

        let expected = short_npub(SHOWCASE_PUBKEY);
        assert_eq!(byline, expected);
        assert!(byline.starts_with("npub1"), "{byline}");
        assert!(
            !byline.starts_with(&SHOWCASE_PUBKEY[..8]),
            "byline must not be a hex prefix: {byline}"
        );
        // The claim still fires — the displaying component owns it.
        assert_eq!(host.claimed.borrow().len(), 1);
    }

    #[test]
    fn author_byline_without_host_uses_npub_short() {
        // Preview-only callers (no host) still get a Rust-formatted npub_short,
        // never the static `author_display_name` and never hex.
        let byline = author_byline(None, None, SHOWCASE_PUBKEY);
        assert_eq!(byline, short_npub(SHOWCASE_PUBKEY));
    }

    #[test]
    fn short_note_renderer_paints_live_resolved_byline() {
        let host = FakeAuthorHost {
            display: Some("pablof7z".to_string()),
            claimed: RefCell::new(Vec::new()),
        };
        // The projection carries ONLY the raw author pubkey (#2514); the byline
        // must come from the live-resolved claim against that pubkey.
        let projection = EmbedKindProjection::ShortNote(ShortNoteProjection {
            id: "b".repeat(64),
            author_pubkey: SHOWCASE_PUBKEY.to_string(),
            created_at: 0,
            content_tree: ContentTreeWire::default(),
            media_urls: Vec::new(),
        });

        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        let registry = NostrKindRegistry::make_default();
        let ctx = nmp_content::RenderContext::new();
        registry.resolve(&projection).render(
            &projection,
            &ctx,
            &registry,
            Some(&host),
            Some("content-kind-registry"),
            area,
            &mut buf,
        );

        let text = buffer_text(&buf);
        assert!(text.contains("pablof7z"), "{text}");
        assert_eq!(host.claimed.borrow().len(), 1);
    }

    #[test]
    fn embedded_event_forwards_author_host_to_renderer() {
        // Reachability: the wired path render.rs → NostrContentView →
        // render_embedded_event → EmbeddedEvent::author_host → KindRenderer.
        // Proves the host actually reaches the byline renderer through the
        // EmbeddedEvent widget, not only the helper in isolation.
        use nmp_content::embed_projection::{EmbeddedEventEnvelope, RenderContextWire};
        use nmp_content::RenderContext;
        use ratatui::widgets::Widget;

        use super::super::super::EmbeddedEvent;

        let host = FakeAuthorHost {
            display: Some("pablof7z".to_string()),
            claimed: RefCell::new(Vec::new()),
        };
        let envelope = EmbeddedEventEnvelope {
            uri: "nostr:nevent1example".to_string(),
            primary_id: "b".repeat(64),
            render_context: RenderContextWire::from(&RenderContext::new()),
            projection: EmbedKindProjection::ShortNote(ShortNoteProjection {
                id: "b".repeat(64),
                author_pubkey: SHOWCASE_PUBKEY.to_string(),
                created_at: 0,
                content_tree: ContentTreeWire::default(),
                media_urls: Vec::new(),
            }),
            collapsed: false,
            collapse_reason: None,
        };

        let area = Rect::new(0, 0, 48, 8);
        let mut buf = Buffer::empty(area);
        let registry = NostrKindRegistry::make_default();
        EmbeddedEvent::new(&envelope, &registry)
            .author_host(Some(&host), Some("content-kind-registry"))
            .render(area, &mut buf);

        let text = buffer_text(&buf);
        assert!(text.contains("pablof7z"), "{text}");
        assert_eq!(host.claimed.borrow().len(), 1);
    }
}
