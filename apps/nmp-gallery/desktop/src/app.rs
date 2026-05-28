//! egui application shell for the desktop component gallery.
//!
//! Mirrors the TUI gallery's layout: sidebar (component registry) + detail
//! (selected component preview). Connected to the in-process kernel via
//! [`GalleryBridge`] so embeds resolve reactively (ADR-0034).

use std::sync::Arc;

use eframe::App;
use egui::{CentralPanel, Color32, ScrollArea, SidePanel, TopBottomPanel};
use nmp_gallery_tui::gallery::REGISTRY_SECTIONS;
use nmp_gallery_tui::{data::GalleryData, embed_host::EmbedHostState};

use crate::bridge::GalleryBridge;
use crate::render::{render_component, EmbedFrameContext};

const CONSUMER_ID: &str = "nmp-gallery-desktop.preview";

pub struct GalleryApp {
    bridge: Arc<GalleryBridge>,
    data: GalleryData,
    host: EmbedHostState,
    selected_index: usize,
    last_rev: u64,
}

impl GalleryApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let bridge = Arc::new(GalleryBridge::start(cc.egui_ctx.clone()));
        let data = GalleryData::render_test_data();
        let host = EmbedHostState::new();

        Self {
            bridge,
            data,
            host,
            selected_index: 0,
            last_rev: 0,
        }
    }
}

impl App for GalleryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain any new snapshots and update the embed host.
        if let Some(value) = self.bridge.snapshot_value() {
            let authors = self.host.update_from_snapshot(&value);
            // Trigger profile claims for authors without resolved kind:0.
            for pubkey in authors {
                self.bridge.claim_profile(&pubkey, CONSUMER_ID);
            }
            // Bump rev for diagnostics.
            self.last_rev += 1;
        }

        self.status_bar(ctx);
        self.sidebar(ctx);
        self.detail(ctx);
    }
}

impl GalleryApp {
    fn status_bar(&self, ctx: &egui::Context) {
        TopBottomPanel::top("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("NMP Desktop Gallery");
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("rev {}", self.last_rev))
                        .monospace()
                        .color(Color32::from_rgb(148, 163, 184)),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("{} embeds", self.host.len()))
                        .monospace()
                        .color(Color32::from_rgb(148, 163, 184)),
                );
            });
        });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        SidePanel::left("sidebar")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Components");
                ui.separator();

                ScrollArea::vertical().show(ui, |ui| {
                    let mut flat_index = 0_usize;
                    for section in REGISTRY_SECTIONS {
                        ui.label(
                            egui::RichText::new(section.label)
                                .strong()
                                .color(Color32::from_rgb(125, 211, 252)),
                        );
                        for component in section.components {
                            let active = flat_index == self.selected_index;
                            let text = egui::RichText::new(component.label).color(if active {
                                Color32::WHITE
                            } else {
                                Color32::from_rgb(203, 213, 225)
                            });
                            if ui.selectable_label(active, text).clicked() {
                                self.selected_index = flat_index;
                            }
                            flat_index += 1;
                        }
                        ui.add_space(8.0);
                    }
                });
            });
    }

    fn detail(&self, ctx: &egui::Context) {
        CentralPanel::default().show(ctx, |ui| {
            let flat: Vec<_> = REGISTRY_SECTIONS
                .iter()
                .flat_map(|s| s.components)
                .collect();
            let component = flat.get(self.selected_index).copied();

            if let Some(spec) = component {
                ui.heading(spec.label);
                ui.label(
                    egui::RichText::new(spec.description)
                        .color(Color32::from_rgb(148, 163, 184))
                        .size(12.0),
                );
                ui.separator();

                let embed_ctx = EmbedFrameContext {
                    envelopes: self.host.current_envelopes(),
                    sink: Some(&*self.bridge),
                    consumer_id: CONSUMER_ID,
                };

                ScrollArea::vertical().show(ui, |ui| {
                    render_component(spec.id, ui, &self.data, embed_ctx);
                });
            } else {
                ui.label("Select a component from the sidebar.");
            }
        });
    }
}
