use egui::{Color32, Ui};

use nmp_gallery_tui::{
    content_render_data::ContentRenderData,
    content_tree_wire::{ContentTreeWire, WireNode},
};

/// Minimal inline content renderer — text, mentions, hashtags, URLs.
///
/// Mirrors `NostrMinimalContent` from the TUI registry. Walks the content
/// tree roots and renders inline nodes as styled egui labels.
pub struct MinimalContent<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
}

impl<'a> MinimalContent<'a> {
    #[must_use]
    pub fn new(tree: &'a ContentTreeWire) -> Self {
        Self {
            tree,
            render_data: None,
        }
    }

    #[must_use]
    pub fn render_data(mut self, render_data: Option<&'a ContentRenderData>) -> Self {
        self.render_data = render_data;
        self
    }

    pub fn show(self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for root in &self.tree.roots {
                append_inline(self.tree, self.render_data, *root, ui);
            }
        });
    }
}

fn append_inline(
    tree: &ContentTreeWire,
    render_data: Option<&ContentRenderData>,
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
        WireNode::Paragraph { children } => {
            for child in children {
                append_inline(tree, render_data, *child, ui);
            }
        }
        WireNode::Emphasis { children } => {
            ui.horizontal(|ui| {
                for child in children {
                    append_inline(tree, render_data, *child, ui);
                }
            });
        }
        WireNode::Strong { children } => {
            ui.horizontal(|ui| {
                for child in children {
                    append_inline(tree, render_data, *child, ui);
                }
            });
        }
        WireNode::Link { children, href } => {
            if let Some(href) = href {
                ui.hyperlink(href.as_str());
            } else {
                for child in children {
                    append_inline(tree, render_data, *child, ui);
                }
            }
        }
        _ => {}
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
    // fallback: truncate npub
    uri.uri
        .strip_prefix("nostr:")
        .unwrap_or(&uri.uri)
        .chars()
        .take(16)
        .collect()
}
