//! Default renderer for kind:9802 — highlights (NIP-84), the
//! `HighlightProjection` variant of `EmbedKindProjection`. Replace via
//! `NostrKindRegistry::set_highlight` for F-CR-10.

use nmp_content::embed_projection::EmbedKindProjection;

use super::super::super::nostr_mention_chip::NostrMentionProfileHost;
use super::super::kind_renderer::{author_byline, KindRenderer};
use super::text_layout::{render_two_line, text_height};
use super::NostrKindRegistry;

/// Default renderer for `HighlightProjection` (kind:9802).
/// Shows highlighted text + source. Replace via `registry.set_highlight(...)` for F-CR-10.
pub struct DefaultHighlightRenderer;

impl KindRenderer for DefaultHighlightRenderer {
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
        let EmbedKindProjection::Highlight(highlight) = projection else {
            return;
        };
        // Component-owned kind:0: self-claiming author byline (iOS PR #833).
        let author = author_byline(host, consumer_id, &highlight.author_pubkey);
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
