//! NostrKindRegistry for the TUI (F-CR-06).
//!
//! Single source of truth for kind → renderer dispatch in the terminal.
//! Renderers live one module per Nostr-kind family under this module (see
//! `short_note`, `article`, `highlight`, `profile`, `unknown`); shared
//! text-layout primitives live in `text_layout`. This file owns only the
//! dispatch table.

use std::collections::HashMap;
use std::sync::Arc;

use nmp_content::embed_projection::EmbedKindProjection;

use super::kind_renderer::{KindRenderer, KindRendererRef};

mod article;
mod highlight;
mod profile;
mod short_note;
mod text_layout;
mod unknown;

pub use article::DefaultArticleRenderer;
pub use highlight::DefaultHighlightRenderer;
pub use profile::DefaultProfileRenderer;
pub use short_note::DefaultShortNoteRenderer;
pub use unknown::DefaultUnknownRenderer;

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nmp_content::{ContentTreeWire, UnknownProjection};
    use ratatui::{buffer::Buffer, layout::Rect};

    use super::super::super::nostr_mention_chip::NostrMentionProfileHost;
    use super::*;

    struct HeightRenderer(u16);

    impl KindRenderer for HeightRenderer {
        fn render(
            &self,
            _projection: &EmbedKindProjection,
            _ctx: &nmp_content::RenderContext,
            _registry: &NostrKindRegistry,
            _host: Option<&dyn NostrMentionProfileHost>,
            _consumer_id: Option<&str>,
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
            created_at: 0,
            content: "hello".to_string(),
            content_tree: ContentTreeWire::default(),
            tags: Vec::new(),
            alt_text: None,
        })
    }
}
