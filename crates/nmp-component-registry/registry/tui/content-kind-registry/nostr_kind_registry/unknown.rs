//! Fallback renderer for numeric Nostr kinds with no registered handler —
//! the `Unknown` variant of `EmbedKindProjection`. Knows nothing about named
//! kind variants; register a specific handler via
//! `NostrKindRegistry::register_unknown` instead of editing this fallback.

use nmp_content::embed_projection::EmbedKindProjection;

use super::super::kind_renderer::{author_byline, KindRenderer};
use super::super::super::nostr_mention_chip::NostrMentionProfileHost;
use super::text_layout::{render_two_line, text_height, tree_text};
use super::NostrKindRegistry;

/// Fallback renderer for `EmbedKindProjection::Unknown` — numeric Nostr kinds
/// that have no registered handler. Knows nothing about named variants.
pub struct DefaultUnknownRenderer;

impl KindRenderer for DefaultUnknownRenderer {
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
        let EmbedKindProjection::Unknown(unknown) = projection else {
            return;
        };
        // Component-owned kind:0: self-claiming author byline (iOS PR #833).
        let author = author_byline(host, consumer_id, &unknown.author_pubkey);
        let body = if unknown.content.is_empty() {
            tree_text(&unknown.content_tree)
        } else {
            unknown.content.clone()
        };
        render_two_line(
            &format!("kind:{} · {author}", unknown.kind),
            &body,
            area,
            buf,
        );
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
