use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use nmp_content::embed_projection::EmbeddedEventEnvelope;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    text::Line,
    widgets::{Paragraph, Widget, Wrap},
};
use ratatui_image::protocol::Protocol;

use crate::{
    content_kind_registry::NostrKindRegistry,
    content_render_data::ContentProfileRenderData,
    content_tree_wire::WireNode,
    data::{article_naddr, ContentExample, GalleryData, LiveProfileMap},
    live::LiveKernelSink,
    nostr_avatar::{NostrAvatar, NostrProfileHost},
    nostr_content_view::NostrContentView,
    nostr_media_grid::NostrMediaGrid,
    nostr_mention_chip::{NostrMentionChip, NostrMentionProfileHost},
    nostr_minimal_content::NostrMinimalContent,
    nostr_nip05_badge::NostrNip05Badge,
    nostr_npub_chip::NostrNpubChip,
    nostr_profile_name::NostrProfileName,
    nostr_user_card::NostrUserCard,
    profile_claim::{ProfileClaim, ProfileClaimShape},
};

/// Per-frame embed-rendering context for renderer-triggered resolve paths.
#[derive(Clone, Copy)]
pub struct EmbedFrameContext<'a> {
    pub envelopes: &'a BTreeMap<String, EmbeddedEventEnvelope>,
    pub sink: Option<&'a LiveKernelSink>,
    pub profile_claims: Option<&'a RefCell<BTreeSet<ProfileClaim>>>,
    pub consumer_id: &'a str,
    /// Reactive profile store; user-* components resolve `ProfileWire` here.
    pub profiles: &'a LiveProfileMap,
}

pub fn plain_lines(
    id: &str,
    data: &GalleryData,
    profiles: &LiveProfileMap,
    width: usize,
) -> Vec<String> {
    let primary = profiles.resolve(&data.primary_pubkey);
    match id {
        "relay-list" => {
            use nmp_app_gallery::showcase;
            let refs = showcase::references();
            refs.relays
                .iter()
                .map(|r| format!("{} [{}]", r.url, r.role))
                .collect()
        }
        "content-core" => content_core_lines(&data.content_core, width),
        "content-minimal" => content_minimal_lines(&data.content_minimal, width),
        "content-view" => content_view_lines(&data.content_view, width),
        "content-mention-chip" => content_view_lines(&data.content_mention_chip, width),
        "content-media-grid" => content_view_lines(&data.content_media_grid, width),
        "content-kind-registry" | "content-quote-card" => {
            content_view_lines(&data.content_quote_card, width)
        }
        "login-block" => crate::login_block::plain_lines(),
        "user-avatar" => vec![format!("avatar {}", primary.initials())],
        "user-name" => vec![primary.display().to_string()],
        "user-nip05" => vec![primary.nip05().unwrap_or("").to_string()],
        "user-npub" => vec![primary.npub_short.clone()],
        "user-card" => vec![
            primary.display().to_string(),
            primary.nip05().unwrap_or("").to_string(),
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
        "relay-list" => render_relay_list(area, buf),
        "content-core" => paragraph(content_core_ratatui_lines(
            &data.content_core,
            area.width as usize,
        ))
        .render(area, buf),
        "content-minimal" => {
            let profile_host = profile_host_from_context(embed_ctx);
            NostrMinimalContent::new(&data.content_minimal.tree)
                .render_data(Some(&data.content_minimal.render_data))
                .profile_host(Some(&profile_host))
                .consumer_id(Some(embed_ctx.consumer_id))
                .render(area, buf)
        }
        "content-view" => {
            let profile_host = profile_host_from_context(embed_ctx);
            let registry = NostrKindRegistry::make_default();
            NostrContentView::new(&data.content_view.tree)
                .render_data(Some(&data.content_view.render_data))
                .media_images(&media_images)
                .kind_registry(Some(&registry))
                .embedded_events(Some(embed_ctx.envelopes))
                .profile_host(Some(&profile_host))
                .event_ref_resolver(
                    embed_ctx
                        .sink
                        .map(|sink| sink as &dyn nmp_content::EventRefResolver),
                )
                .consumer_id(Some(embed_ctx.consumer_id))
                .render(area, buf)
        }
        "content-mention-chip" => {
            let profile_host = profile_host_from_context(embed_ctx);
            render_mention_chip(
                area,
                buf,
                &data.content_mention_chip,
                Some(&profile_host),
                Some(embed_ctx.consumer_id),
            )
        }
        "content-media-grid" => render_media_grid(
            area,
            buf,
            &data.content_media_grid,
            &media_images,
            embed_ctx,
        ),
        "content-kind-registry" | "content-quote-card" => {
            render_embed_showcase("embed-note", area, buf, data, &media_images, embed_ctx)
        }
        "embed-article" | "embed-profile" | "embed-note" | "embed-highlight" => {
            render_embed_showcase(id, area, buf, data, &media_images, embed_ctx)
        }
        "login-block" => crate::login_block::render(area, buf),
        "user-avatar" => render_avatar(area, buf, data, embed_ctx),
        "user-name" => {
            claim_profile_ref(embed_ctx, &data.primary_pubkey, "tui/user-name");
            let primary = embed_ctx.profiles.resolve(&data.primary_pubkey);
            NostrProfileName::new(&primary).render(area, buf)
        }
        "user-nip05" => {
            claim_profile_card(embed_ctx, &data.primary_pubkey, "tui/user-nip05");
            let primary = embed_ctx.profiles.resolve(&data.primary_pubkey);
            if let Some(badge) = NostrNip05Badge::from_profile(&primary) {
                badge.render(area, buf);
            }
        }
        "user-npub" => {
            let primary = embed_ctx.profiles.resolve(&data.primary_pubkey);
            NostrNpubChip::new(&primary).render(chip(area), buf)
        }
        "user-card" => {
            claim_profile_card(embed_ctx, &data.primary_pubkey, "tui/user-card");
            let primary = embed_ctx.profiles.resolve(&data.primary_pubkey);
            NostrUserCard::new(&primary)
                .avatar_image(data.avatar_image_compact.as_ref())
                .render(card(area), buf)
        }
        _ => paragraph(vec![Line::from("Unknown component")]).render(area, buf),
    }
}

fn render_mention_chip(
    area: Rect,
    buf: &mut Buffer,
    example: &ContentExample,
    profile_host: Option<&dyn NostrMentionProfileHost>,
    consumer_id: Option<&str>,
) {
    let Some(uri) = first_mention(example) else {
        return;
    };
    NostrMentionChip::new(uri)
        .profile(example.render_data.profile_for(uri))
        .profile_host(profile_host)
        .consumer_id(consumer_id)
        .render(area, buf);
}

fn render_media_grid(
    area: Rect,
    buf: &mut Buffer,
    _example: &ContentExample,
    media_images: &[(&str, &Protocol)],
    embed_ctx: EmbedFrameContext<'_>,
) {
    if let Some(sink) = embed_ctx.sink {
        nmp_content::EventRefResolver::resolve_event_ref(
            sink,
            article_naddr(),
            embed_ctx.consumer_id,
        );
    }
    let urls = relay_media_urls(embed_ctx.envelopes);
    if urls.is_empty() {
        paragraph(vec![Line::from(
            "Waiting for relay-backed media from the resolved article.",
        )])
        .render(area, buf);
        return;
    }
    NostrMediaGrid::new(&urls, "image")
        .images(media_images)
        .render(area, buf);
}

fn relay_media_urls(envelopes: &BTreeMap<String, EmbeddedEventEnvelope>) -> Vec<String> {
    let mut out = Vec::new();
    for envelope in envelopes.values() {
        match &envelope.projection {
            nmp_content::embed_projection::EmbedKindProjection::Article(article) => {
                if let Some(url) = article
                    .hero_image_url
                    .as_ref()
                    .filter(|url| !url.is_empty())
                {
                    out.push(url.clone());
                }
            }
            nmp_content::embed_projection::EmbedKindProjection::ShortNote(note) => {
                out.extend(note.media_urls.iter().cloned());
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

fn render_avatar(
    area: Rect,
    buf: &mut Buffer,
    data: &GalleryData,
    embed_ctx: EmbedFrameContext<'_>,
) {
    let centered = Rect {
        x: area.x + area.width.saturating_sub(20) / 2,
        y: area.y,
        width: area.width.min(20),
        height: area.height.min(10),
    };
    let profile_host = profile_host_from_context(embed_ctx);
    NostrAvatar::for_pubkey(&data.primary_pubkey, &profile_host)
        .image(data.avatar_image.as_ref())
        .render(centered, buf);
}

fn render_relay_list(area: Rect, buf: &mut Buffer) {
    use nmp_app_gallery::showcase;
    use ratatui::style::Color;
    use ratatui::text::Span;

    let refs = showcase::references();
    let rows: Vec<Line<'static>> = refs
        .relays
        .iter()
        .map(|relay| {
            Line::from(vec![
                Span::styled(
                    "● ".to_string(),
                    ratatui::style::Style::default().fg(Color::Rgb(34, 197, 94)),
                ),
                Span::styled(
                    relay.url.clone(),
                    ratatui::style::Style::default().fg(Color::Rgb(203, 213, 225)),
                ),
                Span::styled(
                    format!("  [{}]", relay.role),
                    ratatui::style::Style::default().fg(Color::Rgb(148, 163, 184)),
                ),
            ])
        })
        .collect();
    paragraph(rows).render(area, buf);
}

fn profile_host_from_context<'a>(embed_ctx: EmbedFrameContext<'a>) -> GalleryProfileHost<'a> {
    GalleryProfileHost {
        sink: embed_ctx.sink,
        profiles: embed_ctx.profiles,
        claims: embed_ctx.profile_claims,
    }
}

struct GalleryProfileHost<'a> {
    sink: Option<&'a LiveKernelSink>,
    profiles: &'a LiveProfileMap,
    claims: Option<&'a RefCell<BTreeSet<ProfileClaim>>>,
}

impl NostrProfileHost for GalleryProfileHost<'_> {
    fn profile_for_pubkey(&self, pubkey: &str) -> crate::profile_wire::ProfileWire {
        self.resolve_profile(pubkey)
    }

    fn resolve_ref(&self, pubkey: &str, consumer_id: &str) {
        self.resolve_profile_ref(pubkey, consumer_id);
    }

    fn release_ref(&self, pubkey: &str, consumer_id: &str) {
        if let Some(sink) = self.sink {
            sink.release_ref(pubkey, consumer_id);
        }
    }
}

impl NostrMentionProfileHost for GalleryProfileHost<'_> {
    fn profile_for_pubkey(&self, pubkey: &str) -> Option<ContentProfileRenderData> {
        let profile = self.resolve_profile(pubkey);
        Some(ContentProfileRenderData {
            pubkey: profile.pubkey,
            display_name: profile.display_name,
            npub: Some(profile.npub),
            picture_url: profile.picture_url,
        })
    }

    fn resolve_ref(&self, pubkey: &str, consumer_id: &str) {
        self.resolve_profile_ref(pubkey, consumer_id);
    }
}

impl GalleryProfileHost<'_> {
    fn resolve_profile(&self, pubkey: &str) -> crate::profile_wire::ProfileWire {
        self.profiles.resolve(pubkey)
    }

    fn resolve_profile_ref(&self, pubkey: &str, consumer_id: &str) {
        claim_profile_ref_parts(self.sink, self.claims, pubkey, consumer_id);
    }
}

fn claim_profile_ref(embed_ctx: EmbedFrameContext<'_>, pubkey: &str, consumer_id: &str) {
    claim_profile_ref_parts(
        embed_ctx.sink,
        embed_ctx.profile_claims,
        pubkey,
        consumer_id,
    );
}

fn claim_profile_card(embed_ctx: EmbedFrameContext<'_>, pubkey: &str, consumer_id: &str) {
    if let Some(claims) = embed_ctx.profile_claims {
        claims.borrow_mut().insert(ProfileClaim {
            pubkey: pubkey.to_string(),
            consumer_id: consumer_id.to_string(),
            shape: ProfileClaimShape::Card,
        });
    }
    if let Some(sink) = embed_ctx.sink {
        sink.resolve_profile_card(pubkey, consumer_id);
    }
}

fn claim_profile_ref_parts(
    sink: Option<&LiveKernelSink>,
    claims: Option<&RefCell<BTreeSet<ProfileClaim>>>,
    pubkey: &str,
    consumer_id: &str,
) {
    if let Some(claims) = claims {
        claims.borrow_mut().insert(ProfileClaim {
            pubkey: pubkey.to_string(),
            consumer_id: consumer_id.to_string(),
            shape: ProfileClaimShape::Ref,
        });
    }
    if let Some(sink) = sink {
        sink.resolve_profile(pubkey, consumer_id);
    }
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

fn first_mention(example: &ContentExample) -> Option<&crate::content_tree_wire::WireUri> {
    example.tree.nodes.iter().find_map(|node| match node {
        WireNode::Mention(uri) => Some(uri),
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
    let profile_host = profile_host_from_context(embed_ctx);

    NostrContentView::new(&example.tree)
        .render_data(Some(&example.render_data))
        .media_images(media_images)
        .kind_registry(Some(&registry))
        .embedded_events(Some(embed_ctx.envelopes))
        .profile_host(Some(&profile_host))
        .event_ref_resolver(
            embed_ctx
                .sink
                .map(|sink| sink as &dyn nmp_content::EventRefResolver),
        )
        .consumer_id(Some(embed_ctx.consumer_id))
        .render(area, buf);
}

#[cfg(test)]
mod tests;
