use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget, Wrap},
};

use super::super::{
    content_kind_registry::EmbeddedEvent,
    content_tree_wire::{WireNode, WireUri},
    nostr_media_grid::NostrMediaGrid,
    ratatui_text_wrap::wrap_spans,
};
use super::{is_inline_root, NostrContentView};

impl Widget for NostrContentView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut cursor = area.y;
        let mut root_pos = 0usize;
        while root_pos < self.tree.roots.len() {
            let root = self.tree.roots[root_pos];
            let Some(node) = self.tree.node(root) else {
                root_pos += 1;
                continue;
            };
            if is_inline_root(node) {
                let inline = self.collect_inline_roots(&mut root_pos);
                self.render_lines(
                    wrap_spans(inline, area.width as usize),
                    area,
                    buf,
                    &mut cursor,
                );
            } else {
                self.render_node(root, area, buf, &mut cursor);
                root_pos += 1;
            }
            if cursor >= area.bottom() {
                break;
            }
        }
        if cursor == area.y {
            Paragraph::new("").render(area, buf);
        }
    }
}

impl NostrContentView<'_> {
    fn render_node(&self, index: usize, area: Rect, buf: &mut Buffer, cursor: &mut u16) {
        let Some(node) = self.tree.node(index) else {
            return;
        };
        match node {
            WireNode::Paragraph { children } => self.render_paragraph(children, area, buf, cursor),
            WireNode::Media { urls, kind } => self.render_media(urls, kind, area, buf, cursor),
            WireNode::Image { src, .. } => {
                if let Some(src) = src {
                    self.render_media(std::slice::from_ref(src), "image", area, buf, cursor);
                }
            }
            WireNode::EventRef(uri) => {
                if !self.render_embedded_event(uri, area, buf, cursor) {
                    self.render_unresolved_embed(uri, area, buf, cursor);
                }
            }
            WireNode::BlockQuote { children } => {
                self.render_blockquote(children, area, buf, cursor);
            }
            _ => {
                let lines = {
                    let mut out = Vec::new();
                    self.append_node(index, area.width as usize, &mut out);
                    out
                };
                self.render_lines(lines, area, buf, cursor);
            }
        }
    }

    fn render_paragraph(&self, children: &[usize], area: Rect, buf: &mut Buffer, cursor: &mut u16) {
        let mut inline = Vec::new();
        for child in children {
            let Some(node) = self.tree.node(*child) else {
                continue;
            };
            match node {
                WireNode::EventRef(uri) => {
                    self.render_lines(
                        wrap_spans(std::mem::take(&mut inline), area.width as usize),
                        area,
                        buf,
                        cursor,
                    );
                    if !self.render_embedded_event(uri, area, buf, cursor) {
                        self.render_node(*child, area, buf, cursor);
                    }
                }
                WireNode::Media { .. } | WireNode::Image { .. } => {
                    self.render_lines(
                        wrap_spans(std::mem::take(&mut inline), area.width as usize),
                        area,
                        buf,
                        cursor,
                    );
                    self.render_node(*child, area, buf, cursor);
                }
                _ => self.append_inline_node(*child, &mut inline),
            }
        }
        self.render_lines(wrap_spans(inline, area.width as usize), area, buf, cursor);
    }

    fn render_embedded_event(
        &self,
        uri: &WireUri,
        area: Rect,
        buf: &mut Buffer,
        cursor: &mut u16,
    ) -> bool {
        let Some(registry) = self.kind_registry else {
            return false;
        };
        // Edge-triggered fetch: when both resolver and consumer are configured,
        // ask the host to resolve this URI before we look up the envelope. The
        // request is independent of cache state — warm cache resolves refcount
        // upstream as a near no-op; cold cache resolves trigger the OneshotApi
        // path and the envelope surfaces in a later snapshot. Dedup per render
        // pass via the seen-set so multiple references to the same URI in one
        // frame collapse into a single host call.
        if let (Some(sink), Some(consumer)) = (self.event_ref_resolver, self.consumer_id) {
            let mut seen = self.claimed_this_frame.borrow_mut();
            if seen.insert(uri.uri.clone()) {
                sink.resolve_event_ref(&uri.uri, consumer);
            }
        }
        let Some(envelope) = self.envelope_for(uri) else {
            return false;
        };
        // Component-owned kind:0 (iOS #833): thread the content view's own
        // presentation-owned profile host into the embed so the byline
        // renderer claims the author's profile and reads the live-resolved
        // name, instead of the static `author_display_name` projection field.
        let widget =
            EmbeddedEvent::new(envelope, registry).author_host(self.profile_host, self.consumer_id);
        let height = widget.preferred_height(area.width);
        let rect = take_area(area, cursor, height);
        if rect.is_empty() {
            return true;
        }
        widget.render(rect, buf);
        *cursor = rect.bottom().saturating_add(1).min(area.bottom());
        true
    }

    fn envelope_for(
        &self,
        uri: &WireUri,
    ) -> Option<&nmp_content::embed_projection::EmbeddedEventEnvelope> {
        let events = self.embedded_events?;
        events.get(&uri.primary_id).or_else(|| events.get(&uri.uri))
    }

    /// Render an unresolved `nostr:` event reference — the kernel has not
    /// shipped its envelope yet (no host wired, or the fetch is in flight). The
    /// resolved render dispatches through the kind-registry `EmbeddedEvent`; this
    /// is the inline placeholder line shown until then.
    fn render_unresolved_embed(
        &self,
        uri: &WireUri,
        area: Rect,
        buf: &mut Buffer,
        cursor: &mut u16,
    ) {
        let lines = self.event_ref_lines(uri, area.width as usize);
        self.render_lines(lines, area, buf, cursor);
    }

    /// Render a markdown blockquote (`> …`). Mirrors the `lines()` path's
    /// `blockquote_lines`; not an embedded-event quote card.
    fn render_blockquote(
        &self,
        children: &[usize],
        area: Rect,
        buf: &mut Buffer,
        cursor: &mut u16,
    ) {
        let lines = self.blockquote_lines(children, area.width as usize);
        self.render_lines(lines, area, buf, cursor);
    }

    fn render_media(
        &self,
        urls: &[String],
        kind: &str,
        area: Rect,
        buf: &mut Buffer,
        cursor: &mut u16,
    ) {
        let grid = NostrMediaGrid::new(urls, kind).images(self.media_images);
        let rect = take_area(area, cursor, grid.preferred_height());
        if rect.is_empty() {
            return;
        }
        grid.render(rect, buf);
        *cursor = rect.bottom().saturating_add(1).min(area.bottom());
    }

    fn render_lines(
        &self,
        lines: Vec<Line<'static>>,
        area: Rect,
        buf: &mut Buffer,
        cursor: &mut u16,
    ) {
        let lines = lines
            .into_iter()
            .filter(|line| line.spans.iter().any(|span| !span.content.is_empty()))
            .collect::<Vec<_>>();
        if lines.is_empty() {
            return;
        }
        let rect = take_area(area, cursor, lines.len() as u16);
        if rect.is_empty() {
            return;
        }
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(rect, buf);
        *cursor = rect.bottom();
    }
}

fn take_area(area: Rect, cursor: &mut u16, wanted_height: u16) -> Rect {
    if *cursor >= area.bottom() || wanted_height == 0 {
        return Rect::new(area.x, area.bottom(), area.width, 0);
    }
    let available = area.bottom().saturating_sub(*cursor);
    Rect {
        x: area.x,
        y: *cursor,
        width: area.width,
        height: wanted_height.min(available),
    }
}
