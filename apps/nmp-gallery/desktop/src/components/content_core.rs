use egui::Ui;

use nmp_gallery_tui::{
    content_render_data::ContentRenderData,
    content_tree_wire::{ContentTreeWire, WireNode},
};

/// Content-core inspector — structural view of the content tree.
///
/// Mirrors `ContentTreeWire` introspection from the TUI registry. Lists
/// node counts, roots, and a sampled walk of the tree.
pub struct ContentCore<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
}

impl<'a> ContentCore<'a> {
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
        ui.label(egui::RichText::new("ContentTreeWire structure").strong());
        ui.label(format!("nodes: {}", self.tree.nodes.len()));
        ui.label(format!("roots: {}", self.tree.roots.len()));
        ui.label(format!(
            "mentioned pubkeys: {}",
            self.tree.mentioned_pubkeys().len()
        ));
        ui.label(format!(
            "event refs: {}",
            self.tree.event_ref_ids().len()
        ));

        ui.add_space(8.0);
        ui.label(egui::RichText::new("Root nodes").strong());
        for (i, root) in self.tree.roots.iter().enumerate() {
            if let Some(node) = self.tree.nodes.get(*root) {
                ui.label(format!("  root {i}: {node:?}"));
            }
        }

        if self.render_data.is_some() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("RenderData attached").strong().size(11.0));
        }
    }
}
