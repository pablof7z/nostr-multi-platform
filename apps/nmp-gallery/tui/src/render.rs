use std::collections::BTreeMap;

use nmp_content::{embed_projection::EmbeddedEventEnvelope, EventClaimSink};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    text::Line,
    widgets::{Paragraph, Widget, Wrap},
};
use ratatui_image::protocol::Protocol;

use crate::{
    content_kind_registry::NostrKindRegistry,
    content_tree_wire::WireNode,
    data::{ContentExample, GalleryData},
    nostr_avatar::NostrAvatar,
    nostr_content_view::NostrContentView,
    nostr_media_grid::NostrMediaGrid,
    nostr_mention_chip::NostrMentionChip,
    nostr_minimal_content::NostrMinimalContent,
    nostr_nip05_badge::NostrNip05Badge,
    nostr_npub_chip::NostrNpubChip,
    nostr_profile_name::NostrProfileName,
    nostr_quote_card::NostrQuoteCard,
    nostr_user_card::NostrUserCard,
};

/// Per-frame embed-rendering context — the renderer's pulled-in deps so
/// it can drive the renderer-triggered claim path (ADR-0034). `envelopes`
/// is the host's current `claimed_events` map (built from the latest
/// snapshot push); `sink` forwards new claims to the kernel; `consumer_id`
/// is the per-consumer key the kernel refcounts under.
#[derive(Clone, Copy)]
pub struct EmbedFrameContext<'a> {
    pub envelopes: &'a BTreeMap<String, EmbeddedEventEnvelope>,
    pub sink: Option<&'a dyn EventClaimSink>,
    pub consumer_id: &'a str,
}


pub fn plain_lines(id: &str, data: &GalleryData, width: usize) -> Vec<String> {
    match id {
        "content-core" => content_core_lines(&data.content_core, width),
        "content-minimal" => content_minimal_lines(&data.content_minimal, width),
        "content-view" => content_view_lines(&data.content_view, width),
        "content-mention-chip" => content_view_lines(&data.content_mention_chip, width),
        "content-media-grid" => content_view_lines(&data.content_media_grid, width),
        "content-quote-card" => quote_card_lines(&data.content_quote_card, width),
        "user-avatar" => vec![format!("avatar {}", data.primary_profile.initials())],
        "user-name" => vec![data.primary_profile.display().to_string()],
        "user-nip05" => vec![data.primary_profile.nip05().unwrap_or("").to_string()],
        "user-npub" => vec![data.primary_profile.npub_short.clone()],
        "user-card" => vec![
            data.primary_profile.display().to_string(),
            data.primary_profile.nip05().unwrap_or("").to_string(),
        ],
        _ => vec![format!("unknown component: {id}")],
    }
}

pub fn render_body(
    id: &str,
    area: Rect,
    buf: &mut Buffer,
    data: &GalleryData,
    embed_ctx: EmbedFrameContext<'_>,
) {
    let media_images = media_refs(data);
    match id {
        "content-core" => paragraph(content_core_ratatui_lines(
            &data.content_core,
            area.width as usize,
        ))
        .render(area, buf),
        "content-minimal" => NostrMinimalContent::new(&data.content_minimal.tree)
            .render_data(Some(&data.content_minimal.render_data))
            .render(area, buf),
        "content-view" => NostrContentView::new(&data.content_view.tree)
            .render_data(Some(&data.content_view.render_data))
            .media_images(&media_images)
            .render(area, buf),
        "content-mention-chip" => render_mention_chip(area, buf, &data.content_mention_chip),
        "content-media-grid" => {
            render_media_grid(area, buf, &data.content_media_grid, &media_images)
        }
        "content-quote-card" => {
            render_quote_card(area, buf, &data.content_quote_card, &media_images)
        }
        "embed-article" | "embed-profile" | "embed-note" | "embed-highlight" => {
            render_embed_showcase(id, area, buf, data, &media_images, embed_ctx)
        }
        "user-avatar" => render_avatar(area, buf, data),
        "user-name" => NostrProfileName::new(&data.primary_profile).render(area, buf),
        "user-nip05" => {
            if let Some(badge) = NostrNip05Badge::from_profile(&data.primary_profile) {
                badge.render(area, buf);
            }
        }
        "user-npub" => NostrNpubChip::new(&data.primary_profile).render(chip(area), buf),
        "user-card" => NostrUserCard::new(&data.primary_profile)
            .avatar_image(data.avatar_image_compact.as_ref())
            .render(card(area), buf),
        _ => paragraph(vec![Line::from("Unknown component")]).render(area, buf),
    }
}

fn render_mention_chip(area: Rect, buf: &mut Buffer, example: &ContentExample) {
    let Some(uri) = first_mention(example) else {
        return;
    };
    NostrMentionChip::new(uri)
        .profile(example.render_data.profile_for(uri))
        .render(area, buf);
}

fn render_media_grid(
    area: Rect,
    buf: &mut Buffer,
    example: &ContentExample,
    media_images: &[(&str, &Protocol)],
) {
    let Some((urls, kind)) = first_media(example) else {
        NostrContentView::new(&example.tree)
            .render_data(Some(&example.render_data))
            .media_images(media_images)
            .render(area, buf);
        return;
    };
    NostrMediaGrid::new(urls, kind)
        .images(media_images)
        .render(area, buf);
}

fn render_quote_card(
    area: Rect,
    buf: &mut Buffer,
    example: &ContentExample,
    media_images: &[(&str, &Protocol)],
) {
    let Some(node) = first_event_ref(example) else {
        return;
    };
    NostrQuoteCard::new(&example.tree, node)
        .render_data(Some(&example.render_data))
        .media_images(media_images)
        .render(area, buf);
}

fn render_avatar(area: Rect, buf: &mut Buffer, data: &GalleryData) {
    let centered = Rect {
        x: area.x + area.width.saturating_sub(20) / 2,
        y: area.y,
        width: area.width.min(20),
        height: area.height.min(10),
    };
    NostrAvatar::new(&data.primary_profile)
        .image(data.avatar_image.as_ref())
        .render(centered, buf);
}

fn content_core_lines(example: &ContentExample, _width: usize) -> Vec<String> {
    vec![
        format!("{} - {}", example.scenario_id, example.title),
        format!("nodes: {}", example.tree.nodes.len()),
        format!("roots: {}", example.tree.roots.len()),
        format!("mentions: {}", example.tree.mentioned_pubkeys().len()),
        format!("event refs: {}", example.tree.event_ref_ids().len()),
    ]
}

fn content_core_ratatui_lines(example: &ContentExample, width: usize) -> Vec<Line<'static>> {
    content_core_lines(example, width)
        .into_iter()
        .map(Line::from)
        .collect()
}

fn content_minimal_lines(example: &ContentExample, width: usize) -> Vec<String> {
    NostrMinimalContent::new(&example.tree)
        .render_data(Some(&example.render_data))
        .lines(width)
        .iter()
        .map(line_text)
        .collect()
}

fn content_view_lines(example: &ContentExample, width: usize) -> Vec<String> {
    NostrContentView::new(&example.tree)
        .render_data(Some(&example.render_data))
        .lines(width)
        .iter()
        .map(line_text)
        .collect()
}

fn quote_card_lines(example: &ContentExample, width: usize) -> Vec<String> {
    first_event_ref(example)
        .map(|node| {
            NostrQuoteCard::new(&example.tree, node)
                .render_data(Some(&example.render_data))
                .lines(width)
                .iter()
                .map(line_text)
                .collect()
        })
        .unwrap_or_default()
}

fn first_mention(example: &ContentExample) -> Option<&crate::content_tree_wire::WireUri> {
    example.tree.nodes.iter().find_map(|node| match node {
        WireNode::Mention(uri) => Some(uri),
        _ => None,
    })
}

fn first_event_ref(example: &ContentExample) -> Option<&WireNode> {
    example
        .tree
        .nodes
        .iter()
        .find(|node| matches!(node, WireNode::EventRef(_)))
}

fn first_media(example: &ContentExample) -> Option<(&[String], &str)> {
    example.tree.nodes.iter().find_map(|node| match node {
        WireNode::Media { urls, kind } => Some((urls.as_slice(), kind.as_str())),
        _ => None,
    })
}

fn paragraph(lines: Vec<Line<'static>>) -> Paragraph<'static> {
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left)
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn chip(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y,
        width: area.width.min(28),
        height: area.height.min(3),
    }
}

fn card(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y,
        width: area.width.min(60),
        height: area.height.min(8),
    }
}

fn media_refs(data: &GalleryData) -> Vec<(&str, &Protocol)> {
    data.media_images
        .iter()
        .map(|image| (image.url.as_str(), &image.protocol))
        .collect()
}

fn render_embed_showcase(
    id: &str,
    area: Rect,
    buf: &mut Buffer,
    data: &GalleryData,
    media_images: &[(&str, &Protocol)],
    embed_ctx: EmbedFrameContext<'_>,
) {
    let example = match id {
        "embed-article" => &data.embed_article,
        "embed-profile" => &data.embed_profile,
        "embed-note" => &data.embed_note,
        "embed-highlight" => &data.embed_highlight,
        _ => &data.content_view,
    };

    let registry = NostrKindRegistry::make_default();

    // M16 / ADR-0034: the renderer is frontend-driven. When `NostrContentView`
    // hits an EventRef(uri), it calls `sink.claim(uri, consumer_id)` — the
    // kernel fetches (cache or relay) and surfaces in `claimed_events`. The
    // `EmbedHostState` decodes that on each snapshot tick and exposes the
    // envelopes through `embed_ctx.envelopes`. The renderer looks them up
    // by `primary_id` / `uri`; if absent → loading placeholder; if present
    // → kind registry dispatches to the right handler.
    NostrContentView::new(&example.tree)
        .render_data(Some(&example.render_data))
        .media_images(media_images)
        .kind_registry(Some(&registry))
        .embedded_events(Some(embed_ctx.envelopes))
        .claim_sink(embed_ctx.sink)
        .consumer_id(Some(embed_ctx.consumer_id))
        .render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_chip_uses_resolved_profile_name() {
        let data = GalleryData::render_test_data();
        let lines = plain_lines("content-mention-chip", &data, 80).join(" ");
        assert!(lines.contains("@Resolved Profile"), "{lines}");
        assert!(!lines.contains("npub1"), "{lines}");
    }

    #[test]
    fn quote_card_uses_event_render_data_instead_of_nevent_text() {
        let data = GalleryData::render_test_data();
        let lines = plain_lines("content-quote-card", &data, 80).join(" ");
        assert!(lines.contains("quote Quoted Author"), "{lines}");
        assert!(
            lines.contains("Quoted event body from render data"),
            "{lines}"
        );
        assert!(!lines.contains("nostr:nevent"), "{lines}");
    }

    #[test]
    fn content_view_projects_nested_mention_preview() {
        let data = GalleryData::render_test_data();
        let lines = NostrContentView::new(&data.content_quote_card.tree)
            .render_data(Some(&data.content_quote_card.render_data))
            .lines(100)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(lines.contains("quote Quoted Author"), "{lines}");
        assert!(
            lines.contains("Quoted event body from render data"),
            "{lines}"
        );
    }

    // Embed-envelope projection tests live in `embed_host::tests` now —
    // they exercise the snapshot → ClaimedEventDto → EmbedKindProjection
    // dispatch (the same path the renderer takes), not a static field on
    // `ContentExample` (which no longer exists). The renderer's
    // `embedded_events(...)` is sourced from `EmbedFrameContext`, not from
    // `ContentExample`.
}
