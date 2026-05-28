use std::collections::BTreeMap;

use egui::{Color32, Ui};
use nmp_content::{embed_projection::EmbeddedEventEnvelope, EventClaimSink};

use nmp_gallery_tui::{
    content_tree_wire::WireNode,
    data::{ContentExample, GalleryData},
};

use crate::components::{
    content_core::ContentCore,
    content_minimal::MinimalContent,
    content_view::ContentView,
    user_avatar::UserAvatar,
    user_card::UserCard,
    user_name::UserName,
    user_nip05::Nip05Badge,
    user_npub::NpubChip,
};

/// Per-frame embed-rendering context for the desktop gallery.
///
/// Mirrors `EmbedFrameContext` from the TUI registry. `envelopes` holds
/// the latest `claimed_events` decoded by `EmbedHostState`; `sink` forwards
/// new claims to the in-process kernel; `consumer_id` is the per-consumer
/// refcount key.
#[derive(Clone, Copy)]
pub struct EmbedFrameContext<'a> {
    pub envelopes: &'a BTreeMap<String, EmbeddedEventEnvelope>,
    pub sink: Option<&'a dyn EventClaimSink>,
    pub consumer_id: &'a str,
}

/// Render the body of the named component into the given [`Ui`].
pub fn render_component(
    id: &str,
    ui: &mut Ui,
    data: &GalleryData,
    embed_ctx: EmbedFrameContext<'_>,
) {
    match id {
        "content-core" => {
            ContentCore::new(&data.content_core.tree)
                .render_data(Some(&data.content_core.render_data))
                .show(ui);
        }
        "content-minimal" => {
            MinimalContent::new(&data.content_minimal.tree)
                .render_data(Some(&data.content_minimal.render_data))
                .show(ui);
        }
        "content-view" => {
            ContentView::new(&data.content_view.tree)
                .render_data(Some(&data.content_view.render_data))
                .show(ui);
        }
        "content-mention-chip" => {
            render_mention_chip(ui, &data.content_mention_chip, embed_ctx);
        }
        "content-media-grid" => {
            render_media_grid(ui, &data.content_media_grid);
        }
        "content-quote-card" => {
            render_quote_card(ui, &data.content_quote_card);
        }
        "embed-article" | "embed-profile" | "embed-note" | "embed-highlight" => {
            render_embed_showcase(id, ui, data, embed_ctx);
        }
        "user-avatar" => {
            ui.vertical_centered(|ui| {
                UserAvatar::new(&data.primary_profile).size(64.0).show(ui);
            });
        }
        "user-name" => {
            UserName::new(&data.primary_profile).show(ui);
        }
        "user-nip05" => {
            if let Some(badge) = Nip05Badge::from_profile(&data.primary_profile) {
                badge.show(ui);
            }
        }
        "user-npub" => {
            NpubChip::new(&data.primary_profile).show(ui);
        }
        "user-card" => {
            UserCard::new(&data.primary_profile).show(ui);
        }
        _ => {
            ui.label("Unknown component");
        }
    }
}

fn render_mention_chip(
    ui: &mut Ui,
    example: &ContentExample,
    embed_ctx: EmbedFrameContext<'_>,
) {
    let Some(uri) = first_mention(example) else { return };

    // Drive the claim path (ADR-0034): if the renderer has a sink,
    // claim the mention URI so the kernel resolves it.
    if let Some(sink) = embed_ctx.sink {
        sink.claim(&uri.uri, embed_ctx.consumer_id);
    }

    let name = if let Some(p) = example.render_data.profile_for(uri) {
        p.display_name
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or("mention")
            .to_string()
    } else if let Some(env) = embed_ctx.envelopes.get(&uri.primary_id) {
        match &env.projection {
            nmp_content::embed_projection::EmbedKindProjection::Profile(p) => {
                p.display_name
                    .as_deref()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or("profile")
                    .to_string()
            }
            _ => "mention".to_string(),
        }
    } else {
        "mention".to_string()
    };

    ui.label(
        egui::RichText::new(format!("@{name}"))
            .color(Color32::from_rgb(96, 165, 250)),
    );
}

fn render_media_grid(ui: &mut Ui, example: &ContentExample) {
    let Some((urls, _kind)) = first_media(example) else {
        ContentView::new(&example.tree)
            .render_data(Some(&example.render_data))
            .show(ui);
        return;
    };

    ui.horizontal_wrapped(|ui| {
        for url in urls {
            ui.hyperlink_to(format!("[media]"), url.as_str());
        }
    });
}

fn render_quote_card(ui: &mut Ui, example: &ContentExample) {
    let Some(_node) = first_event_ref(example) else { return };

    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Quote").strong().size(14.0));
        ui.add_space(4.0);
        ContentView::new(&example.tree)
            .render_data(Some(&example.render_data))
            .show(ui);
    });
}

fn render_embed_showcase(
    id: &str,
    ui: &mut Ui,
    data: &GalleryData,
    embed_ctx: EmbedFrameContext<'_>,
) {
    let example = match id {
        "embed-article" => &data.embed_article,
        "embed-profile" => &data.embed_profile,
        "embed-note" => &data.embed_note,
        "embed-highlight" => &data.embed_highlight,
        _ => &data.content_view,
    };

    // Drive claims for any EventRef nodes in the content tree.
    if let Some(sink) = embed_ctx.sink {
        for node in &example.tree.nodes {
            if let WireNode::EventRef(uri) = node {
                sink.claim(&uri.uri, embed_ctx.consumer_id);
            }
            if let WireNode::Mention(uri) = node {
                sink.claim(&uri.uri, embed_ctx.consumer_id);
            }
        }
    }

    // ContentView renders EventRef nodes inline when a resolved envelope exists.
    ContentView::new(&example.tree)
        .render_data(Some(&example.render_data))
        .embedded_events(Some(embed_ctx.envelopes))
        .show(ui);

    // Show resolved embed envelopes below.
    if !embed_ctx.envelopes.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Resolved embeds:").strong().size(12.0));
        for (primary_id, envelope) in embed_ctx.envelopes {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(primary_id.as_str())
                        .monospace()
                        .size(10.0)
                        .color(Color32::from_rgb(148, 163, 184)),
                );
                let kind_label = match &envelope.projection {
                    nmp_content::embed_projection::EmbedKindProjection::Article(_) => {
                        "article"
                    }
                    nmp_content::embed_projection::EmbedKindProjection::ShortNote(_) => {
                        "note"
                    }
                    nmp_content::embed_projection::EmbedKindProjection::Highlight(_) => {
                        "highlight"
                    }
                    nmp_content::embed_projection::EmbedKindProjection::Profile(_) => {
                        "profile"
                    }
                    nmp_content::embed_projection::EmbedKindProjection::Unknown(_) => {
                        "unknown"
                    }
                };
                ui.label(
                    egui::RichText::new(kind_label)
                        .size(10.0)
                        .color(Color32::from_rgb(110, 231, 183)),
                );
            });
        }
    }
}

fn first_mention(example: &ContentExample) -> Option<&nmp_gallery_tui::content_tree_wire::WireUri> {
    example
        .tree
        .nodes
        .iter()
        .find_map(|node| match node {
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

fn first_media(example: &ContentExample) -> Option<(&Vec<String>, &str)> {
    example.tree.nodes.iter().find_map(|node| match node {
        WireNode::Media { urls, kind } => Some((urls, kind.as_str())),
        _ => None,
    })
}
