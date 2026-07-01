//! EmbeddedEvent — the widget that renders any kind via the registry (F-CR-06).
//!
//! Receives an `EmbeddedEventEnvelope` (from F-CR-01), consults the
//! `NostrKindRegistry`, and wraps the chosen renderer in `EmbedChromeContainer`.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};

use nmp_content::embed_projection::EmbeddedEventEnvelope;

use super::super::nostr_mention_chip::NostrMentionProfileHost;
use super::embed_chrome_container::EmbedChromeContainer;
use super::NostrKindRegistry;

pub struct EmbeddedEvent<'a> {
    pub envelope: &'a EmbeddedEventEnvelope,
    pub registry: &'a NostrKindRegistry,
    /// Presentation-owned profile host the byline renderer claims through
    /// (component-owned kind:0, iOS #833). Threaded from the content view's
    /// own `profile_host`; `None` for preview-only callers, which fall back to
    /// `npub_short`. The chosen kind renderer issues the claim itself.
    author_host: Option<&'a dyn NostrMentionProfileHost>,
    consumer_id: Option<&'a str>,
}

impl<'a> EmbeddedEvent<'a> {
    pub fn new(envelope: &'a EmbeddedEventEnvelope, registry: &'a NostrKindRegistry) -> Self {
        Self {
            envelope,
            registry,
            author_host: None,
            consumer_id: None,
        }
    }

    /// Wire the presentation-owned profile host + consumer id so the byline
    /// renderer can claim the author's kind:0 and read the live-resolved name.
    pub fn author_host(
        mut self,
        host: Option<&'a dyn NostrMentionProfileHost>,
        consumer_id: Option<&'a str>,
    ) -> Self {
        self.author_host = host;
        self.consumer_id = consumer_id;
        self
    }

    pub fn preferred_height(&self, width: u16) -> u16 {
        if self.envelope.collapsed {
            return 1;
        }
        let inner_width = width.saturating_sub(2);
        let renderer = self.registry.resolve(&self.envelope.projection);
        renderer
            .preferred_height(&self.envelope.projection, inner_width)
            .max(1)
    }
}

impl Widget for EmbeddedEvent<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let chrome =
            EmbedChromeContainer::new(self.envelope.render_context.depth, self.envelope.collapsed);

        chrome.render(area, buf);
        let inner = chrome.inner(area);

        if self.envelope.collapsed {
            let reason = self
                .envelope
                .collapse_reason
                .as_deref()
                .unwrap_or("collapsed");
            Line::styled(
                format!("embedded event {reason}"),
                Style::default().fg(Color::Rgb(148, 163, 184)),
            )
            .render(inner, buf);
            return;
        }

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let renderer = self.registry.resolve(&self.envelope.projection);

        // Convert the wire RenderContextWire back to the in-memory RenderContext
        // for any recursive rendering the kind renderer may do.
        let ctx: nmp_content::context::RenderContext = (&self.envelope.render_context).into();

        renderer.render(
            &self.envelope.projection,
            &ctx,
            self.registry,
            self.author_host,
            self.consumer_id,
            inner,
            buf,
        );
    }
}
