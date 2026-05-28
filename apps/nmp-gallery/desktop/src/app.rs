//! egui application shell for the desktop component gallery.
//!
//! Mirrors the TUI gallery's layout: sidebar (component registry) + detail
//! (selected component preview). Connected to the in-process kernel via
//! [`GalleryBridge`] so embeds resolve reactively (ADR-0034).

use std::sync::Arc;

use eframe::App;
use egui::{CentralPanel, Color32, ScrollArea, SidePanel, TopBottomPanel};
use nmp_gallery_tui::gallery::REGISTRY_SECTIONS;
use nmp_gallery_tui::{
    data::GalleryData,
    embed_host::EmbedHostState,
    live::{LiveFacts, LiveItem, LiveProfile},
};

use crate::bridge::GalleryBridge;
use crate::render::{render_component, EmbedFrameContext};

const CONSUMER_ID: &str = "nmp-gallery-desktop.preview";
const PRIMARY_PUBKEY: &str = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
const MENTION_EVENT_ID: &str = "caef905a1e1520fd6621b56364cca823c262327a32ac063b4ff0435f41aa7660";
const MEDIA_EVENT_ID: &str = "c2ee64b0371f290edf66fc797598b2d307aa79192f6d6e0bf5344cf81104029b";
const QUOTE_SOURCE_EVENT_ID: &str =
    "2df88accbf264b10f47809abcf9d32b4146b035a5a197c9ff30e45ac010d5368";

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
        let facts = synthetic_facts();
        let data = GalleryData::from_live(&facts, false).expect("gallery data valid");
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

/// Synthetic `LiveFacts` — deterministic cold-start data that matches the
/// TUI bootstrap's offline-fallback values. Embeds are NOT pre-warmed;
/// the renderer-triggered claim path drives them reactively (ADR-0034).
fn synthetic_facts() -> LiveFacts {
    let primary = LiveProfile {
        pubkey: PRIMARY_PUBKEY.to_string(),
        display_name: Some("Primary Author".to_string()),
        picture_url: Some("https://example.invalid/avatar.png".to_string()),
        nip05: Some("primary.example".to_string()),
        about: Some("Primary author for gallery showcase".to_string()),
    };
    let mention = LiveProfile {
        pubkey: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        display_name: Some("Resolved Profile".to_string()),
        picture_url: Some("https://example.invalid/profile.png".to_string()),
        nip05: Some("resolved.example".to_string()),
        about: Some("Test-only resolved profile".to_string()),
    };
    let quote_target = LiveProfile {
        pubkey: PRIMARY_PUBKEY.to_string(),
        display_name: Some("Quoted Author".to_string()),
        picture_url: None,
        nip05: None,
        about: None,
    };

    let mention_uri = format!(
        "nostr:{}",
        nmp_core::display::to_npub(
            "1111111111111111111111111111111111111111111111111111111111111111"
        )
    );
    let quote_uri = format!(
        "nostr:{}",
        nmp_core::nip19::format(&nmp_core::nip19::Nip19Entity::Note(
            "3333333333333333333333333333333333333333333333333333333333333333".to_string()
        ))
        .expect("note id formats")
    );

    let mention_item = LiveItem {
        id: MENTION_EVENT_ID.to_string(),
        author_pubkey: PRIMARY_PUBKEY.to_string(),
        kind: 1,
        content: format!("hello {mention_uri}"),
        content_preview: String::new(),
        created_at: 1,
    };
    let media_item = LiveItem {
        id: MEDIA_EVENT_ID.to_string(),
        author_pubkey: PRIMARY_PUBKEY.to_string(),
        kind: 1,
        content: "Check out this image https://example.invalid/image1.png and this one https://example.invalid/image2.png".to_string(),
        content_preview: String::new(),
        created_at: 2,
    };
    let quote_source = LiveItem {
        id: QUOTE_SOURCE_EVENT_ID.to_string(),
        author_pubkey: PRIMARY_PUBKEY.to_string(),
        kind: 1,
        content: format!("look {quote_uri}"),
        content_preview: String::new(),
        created_at: 3,
    };
    let quote_target_item = LiveItem {
        id: "3333333333333333333333333333333333333333333333333333333333333333".to_string(),
        author_pubkey: PRIMARY_PUBKEY.to_string(),
        kind: 1,
        content: "Quoted event body from render data".to_string(),
        content_preview: String::new(),
        created_at: 4,
    };

    LiveFacts {
        primary_profile: primary,
        mention_profile: mention,
        quote_target_profile: quote_target,
        mention_item,
        media_item,
        quote_source_item: quote_source,
        quote_target_item,
        mention_profile_uri: mention_uri,
        quote_event_uri: quote_uri,
    }
}
