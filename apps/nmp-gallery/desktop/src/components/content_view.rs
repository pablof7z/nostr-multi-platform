use std::collections::BTreeMap;

use egui::{Color32, Ui};

use nmp_content::embed_projection::{EmbeddedEventEnvelope, EmbedKindProjection};
use nmp_gallery_tui::{
    content_render_data::ContentRenderData,
    content_tree_wire::{ContentTreeWire, WireNode},
};

use crate::components::embed_article::{tree_text, ArticleCard};

/// Full rich content renderer with inline embed support.
///
/// Mirrors `NostrContentView` from the TUI registry. Walks the content tree
/// and renders paragraphs, headings, blockquotes, lists, code blocks, and
/// inline elements with egui styling. When `embedded_events` is supplied,
/// `EventRef` nodes render the resolved embed card inline (not a link).
pub struct ContentView<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
    embedded_events: Option<&'a BTreeMap<String, EmbeddedEventEnvelope>>,
}

impl<'a> ContentView<'a> {
    #[must_use]
    pub fn new(tree: &'a ContentTreeWire) -> Self {
        Self {
            tree,
            render_data: None,
            embedded_events: None,
        }
    }

    #[must_use]
    pub fn render_data(mut self, render_data: Option<&'a ContentRenderData>) -> Self {
        self.render_data = render_data;
        self
    }

    #[must_use]
    pub fn embedded_events(
        mut self,
        events: Option<&'a BTreeMap<String, EmbeddedEventEnvelope>>,
    ) -> Self {
        self.embedded_events = events;
        self
    }

    pub fn show(self, ui: &mut Ui) {
        for root in &self.tree.roots {
            render_node(self.tree, self.render_data, self.embedded_events, *root, ui);
        }
    }
}

fn render_node(
    tree: &ContentTreeWire,
    render_data: Option<&ContentRenderData>,
    embedded: Option<&BTreeMap<String, EmbeddedEventEnvelope>>,
    index: usize,
    ui: &mut Ui,
) {
    let Some(node) = tree.nodes.get(index) else { return };
    match node {
        WireNode::Paragraph { children } => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for child in children {
                    render_inline(tree, render_data, embedded, *child, ui);
                }
            });
        }
        WireNode::Heading { level, children } => {
            let size = match level {
                1 => 24.0,
                2 => 20.0,
                3 => 18.0,
                _ => 16.0,
            };
            ui.horizontal_wrapped(|ui| {
                for child in children {
                    if let Some(WireNode::Text(text)) = tree.nodes.get(*child) {
                        ui.label(egui::RichText::new(text.as_str()).strong().size(size));
                    }
                }
            });
        }
        WireNode::BlockQuote { children } => {
            ui.horizontal(|ui| {
                let color = Color32::from_rgb(148, 163, 184);
                ui.label(egui::RichText::new("|").color(color).size(14.0));
                ui.vertical(|ui| {
                    for child in children {
                        render_node(tree, render_data, embedded, *child, ui);
                    }
                });
            });
        }
        WireNode::List { ordered_start, items } => {
            for (i, item) in items.iter().enumerate() {
                ui.horizontal(|ui| {
                    let bullet = if let Some(start) = ordered_start {
                        format!("{}.", start + i as u64)
                    } else {
                        "•".to_string()
                    };
                    ui.label(bullet);
                    ui.vertical(|ui| {
                        for child in item {
                            render_inline(tree, render_data, embedded, *child, ui);
                        }
                    });
                });
            }
        }
        WireNode::CodeBlock { info, body } => {
            let label = if let Some(info) = info {
                format!("```{info}")
            } else {
                "```".to_string()
            };
            ui.label(egui::RichText::new(label).weak().monospace());
            ui.label(egui::RichText::new(body.as_str()).monospace().size(12.0));
            ui.label(egui::RichText::new("```").weak().monospace());
        }
        WireNode::Rule => {
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
        }
        WireNode::Text(text) => {
            ui.label(text.as_str());
        }
        WireNode::Mention(uri) => {
            let name = resolved_name(render_data, uri);
            ui.label(
                egui::RichText::new(format!("@{name}"))
                    .color(Color32::from_rgb(96, 165, 250)),
            );
        }
        WireNode::EventRef(uri) => {
            // Inline embed: if the envelope is resolved, render the card.
            if let Some(embedded) = embedded {
                if try_render_embed_inline(ui, &uri.primary_id, embedded) {
                    return;
                }
            }
            ui.label(
                egui::RichText::new(format!("nostr:{}", short_id(&uri.primary_id)))
                    .color(Color32::from_rgb(110, 231, 183)),
            );
        }
        WireNode::Hashtag(tag) => {
            ui.label(
                egui::RichText::new(format!("#{tag}"))
                    .color(Color32::from_rgb(96, 165, 250)),
            );
        }
        WireNode::Url(url) => {
            ui.hyperlink(url.as_str());
        }
        WireNode::Media { urls, .. } => {
            for url in urls {
                ui.hyperlink_to("[media]", url.as_str());
            }
        }
        WireNode::Emoji { shortcode, .. } => {
            ui.label(format!(":{shortcode}:"));
        }
        WireNode::Invoice { .. } => {
            ui.label(
                egui::RichText::new("[invoice]")
                    .color(Color32::from_rgb(251, 191, 36)),
            );
        }
        WireNode::SoftBreak | WireNode::HardBreak => {}
        WireNode::Placeholder { reason } => {
            ui.label(
                egui::RichText::new(format!("[placeholder: {reason:?}]"))
                    .weak()
                    .italics(),
            );
        }
        WireNode::Unsupported => {
            ui.label(egui::RichText::new("[unsupported]").weak());
        }
        _ => {}
    }
}

fn render_inline(
    tree: &ContentTreeWire,
    render_data: Option<&ContentRenderData>,
    embedded: Option<&BTreeMap<String, EmbeddedEventEnvelope>>,
    index: usize,
    ui: &mut Ui,
) {
    let Some(node) = tree.nodes.get(index) else { return };
    match node {
        WireNode::Text(text) => {
            ui.label(text.as_str());
        }
        WireNode::Mention(uri) => {
            let name = resolved_name(render_data, uri);
            ui.label(
                egui::RichText::new(format!("@{name}"))
                    .color(Color32::from_rgb(96, 165, 250)),
            );
        }
        WireNode::Hashtag(tag) => {
            ui.label(
                egui::RichText::new(format!("#{tag}"))
                    .color(Color32::from_rgb(96, 165, 250)),
            );
        }
        WireNode::Url(url) => {
            ui.hyperlink(url.as_str());
        }
        WireNode::Media { urls, .. } => {
            for url in urls {
                ui.hyperlink_to("[media]", url.as_str());
            }
        }
        WireNode::Emoji { shortcode, .. } => {
            ui.label(format!(":{shortcode}:"));
        }
        WireNode::Invoice { .. } => {
            ui.label(
                egui::RichText::new("[invoice]")
                    .color(Color32::from_rgb(251, 191, 36)),
            );
        }
        WireNode::SoftBreak => {}
        WireNode::HardBreak => {
            ui.add_space(4.0);
        }
        WireNode::Emphasis { children } => {
            for child in children {
                if let Some(WireNode::Text(text)) = tree.nodes.get(*child) {
                    ui.label(egui::RichText::new(text.as_str()).italics());
                }
            }
        }
        WireNode::Strong { children } => {
            for child in children {
                if let Some(WireNode::Text(text)) = tree.nodes.get(*child) {
                    ui.label(egui::RichText::new(text.as_str()).strong());
                }
            }
        }
        WireNode::InlineCode(code) => {
            ui.label(egui::RichText::new(code.as_str()).monospace());
        }
        WireNode::Link { children, href } => {
            if let Some(href) = href {
                ui.hyperlink(href.as_str());
            } else {
                for child in children {
                    render_inline(tree, render_data, embedded, *child, ui);
                }
            }
        }
        WireNode::Image { alt, src, .. } => {
            if let Some(src) = src {
                ui.hyperlink_to(format!("[img: {alt}]"), src.as_str());
            } else {
                ui.label(format!("[img: {alt}]"));
            }
        }
        WireNode::EventRef(uri) => {
            if let Some(embedded) = embedded {
                if try_render_embed_inline(ui, &uri.primary_id, embedded) {
                    return;
                }
            }
            ui.label(
                egui::RichText::new(format!("nostr:{}", short_id(&uri.primary_id)))
                    .color(Color32::from_rgb(110, 231, 183)),
            );
        }
        _ => {}
    }
}

fn try_render_embed_inline(
    ui: &mut Ui,
    primary_id: &str,
    envelopes: &BTreeMap<String, EmbeddedEventEnvelope>,
) -> bool {
    let Some(envelope) = envelopes.get(primary_id) else {
        return false;
    };
    match &envelope.projection {
        EmbedKindProjection::Article(ref article) => {
            ArticleCard::new(article).show(ui);
            true
        }
        EmbedKindProjection::ShortNote(ref note) => {
            ui.vertical(|ui| {
                let author = note
                    .author_display_name
                    .as_deref()
                    .unwrap_or(&note.author_pubkey[..8.min(note.author_pubkey.len())]);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(author)
                            .strong()
                            .size(12.0)
                            .color(Color32::from_rgb(203, 213, 225)),
                    );
                });
                let preview = tree_text(&note.content_tree);
                ui.label(
                    egui::RichText::new(truncate_chars(&preview, 200))
                        .size(12.0)
                        .color(Color32::from_rgb(148, 163, 184)),
                );
            });
            true
        }
        EmbedKindProjection::Highlight(ref h) => {
            ui.vertical(|ui| {
                let color = Color32::from_rgb(251, 191, 36);
                ui.label(egui::RichText::new(&h.highlighted_text).color(color).size(12.0));
                if let Some(url) = &h.source_url {
                    ui.hyperlink_to("source", url.as_str());
                }
            });
            true
        }
        EmbedKindProjection::Profile(ref p) => {
            let name = p
                .display_name
                .as_deref()
                .unwrap_or(&p.pubkey[..8.min(p.pubkey.len())]);
            ui.label(
                egui::RichText::new(format!("@{name}"))
                    .color(Color32::from_rgb(96, 165, 250))
                    .size(13.0),
            );
            true
        }
        EmbedKindProjection::Unknown(_) => false,
    }
}

fn resolved_name(render_data: Option<&ContentRenderData>, uri: &nmp_gallery_tui::content_tree_wire::WireUri) -> String {
    if let Some(data) = render_data {
        if let Some(p) = data.profile_for(uri) {
            if let Some(ref name) = p.display_name {
                if !name.trim().is_empty() {
                    return name.clone();
                }
            }
        }
    }
    uri.uri
        .strip_prefix("nostr:")
        .unwrap_or(&uri.uri)
        .chars()
        .take(16)
        .collect()
}

fn short_id(id: &str) -> String {
    if id.len() > 16 {
        format!("{}\u{2026}{}", &id[..8], &id[id.len() - 8..])
    } else {
        id.to_string()
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
