//! Home timeline rendering for Chirp Desktop.
//!
//! Kept out of `app.rs` so timeline/card UI can evolve without growing the
//! top-level shell past the repository file-size ceiling.

use std::collections::HashMap;

use egui::{Align, Color32, Frame, Layout, RichText, ScrollArea, Ui};
use nmp_core::tags::Nip10Refs;
use nmp_nip01::NoteRecord;

use crate::app::{AppTab, DesktopApp};
use crate::bridge::AppRuntime;
use crate::render::{hex_color, note_body};
use crate::snapshot::{ModularTimelineSnapshot, ProfileCard, Snapshot, TimelineEventCard};
use crate::zap_amount::PendingZap;

impl DesktopApp {
    pub(crate) fn timeline(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let feed: ModularTimelineSnapshot = snap.projection("nmp.feed.home").unwrap_or_default();
        // ADR-0063 (#1671 Lane F): read author/mention profiles from the
        // refs.profile mirror (resolve_ref output) instead of resolved_profiles.
        let profiles: HashMap<String, ProfileCard> = snap.refs_profiles.clone();

        if feed.cards.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(RichText::new("Connecting to relays…").size(15.0).weak());
                ui.label(RichText::new("Live timeline will appear here.").weak());
            });
            return;
        }

        let mut nav: Option<AppTab> = None;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &feed.cards {
                    if let Some(target) = feed_card(
                        ui,
                        &entry.card,
                        &profiles,
                        &snap.embeds,
                        &mut nav,
                        &mut self.reply_to,
                        &self.bridge,
                    ) {
                        self.zap_amount.open(target);
                    }
                    ui.add_space(6.0);
                }

                if feed.page.as_ref().is_some_and(|page| page.has_more) {
                    ui.add_space(8.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Load Older").clicked() {
                            self.bridge.load_older_timeline();
                        }
                    });
                }
            });
        // ADR-0063 (#1671 Lane F): apply a requested transition through the
        // single navigation chokepoint (releases the outgoing view's ref).
        if let Some(next) = nav {
            self.navigate_to(next);
        }
    }
}

fn display_label(pubkey: &str, profiles: &HashMap<String, ProfileCard>) -> String {
    profiles
        .get(pubkey)
        .and_then(|p| p.display_name.as_deref())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| nmp_core::display::short_npub(pubkey))
}

/// Render one feed card. Navigation (author/thread open) is NOT performed here:
/// a requested tab transition is written to `nav` so the caller can route it
/// through the single `DesktopApp::navigate_to` chokepoint (ADR-0063 #1671 Lane
/// F — releasing the outgoing Author/Thread view's ref before opening the next).
pub(crate) fn feed_card(
    ui: &mut Ui,
    card: &TimelineEventCard,
    profiles: &HashMap<String, ProfileCard>,
    embeds: &HashMap<String, nmp_content::EmbeddedEventEnvelope>,
    nav: &mut Option<AppTab>,
    reply_to: &mut Option<NoteRecord>,
    bridge: &AppRuntime,
) -> Option<PendingZap> {
    let author_display = display_label(&card.author_pubkey, profiles);
    let initials =
        nmp_core::display::avatar_initials(&nmp_core::display::to_npub(&card.author_pubkey));
    let color = nmp_core::display::avatar_color_hex(&card.author_pubkey);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let created_at_display = nmp_core::display::format_ago_secs(now, card.created_at);
    let mut zap_target = None;

    Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                avatar(ui, &initials, &color);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    if let Some(repost) = &card.reposted_by {
                        let reposter = display_label(&repost.author_pubkey, profiles);
                        ui.label(
                            RichText::new(format!("↻ reposted by {reposter}"))
                                .small()
                                .weak()
                                .color(Color32::from_rgb(148, 163, 184)),
                        );
                    }
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new(&author_display).strong()).clicked() {
                            *nav = Some(AppTab::Author(card.author_pubkey.clone()));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(&created_at_display).weak().small());
                        });
                    });
                    if card.reposted_by.is_some() {
                        ui.label(
                            RichText::new("↩ repost")
                                .small()
                                .weak()
                                .color(Color32::from_rgb(148, 163, 184)),
                        );
                    }
                    let text = &card.content;
                    if !text.is_empty() {
                        let scope = ui.scope(|ui| {
                            note_body(ui, text.as_ref(), embeds, profiles);
                        });
                        if scope.response.interact(egui::Sense::click()).clicked() {
                            *nav = Some(AppTab::Thread(card.id.clone()));
                        }
                    }
                    ui.label(
                        RichText::new(card.relation_counts.summary())
                            .small()
                            .weak()
                            .color(Color32::from_rgb(148, 163, 184)),
                    );
                    if !card.relay_provenance.is_empty() {
                        let relay_count = card.relay_provenance.len();
                        let label = if relay_count == 1 {
                            "Received from 1 relay".to_string()
                        } else {
                            format!("Received from {relay_count} relays")
                        };
                        if ui
                            .small_button(label)
                            .on_hover_text(card.relay_provenance.join("\n"))
                            .clicked()
                        {
                            ui.ctx().copy_text(card.relay_provenance.join("\n"));
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("↩ Reply").clicked() {
                            *reply_to = Some(note_record_from_card(card));
                        }
                        if ui.small_button("❤ Like").clicked() {
                            let _ = bridge.react(&card.id, "+");
                        }
                        if ui.small_button("🔁 Repost").clicked() {
                            let _ = bridge.repost(&card.id, &card.author_pubkey);
                        }
                        if ui.small_button("⚡ Zap").clicked() && !card.author_pubkey.is_empty() {
                            zap_target =
                                Some(PendingZap::new(card.author_pubkey.clone(), card.id.clone()));
                        }
                    });
                });
            });
        });

    zap_target
}

pub(crate) fn note_record_from_card(card: &TimelineEventCard) -> NoteRecord {
    NoteRecord {
        event_id: card.id.clone(),
        author: card.author_pubkey.clone(),
        created_at: card.created_at,
        content: card.content.clone(),
        refs: Nip10Refs::default(),
    }
}

pub(crate) fn avatar(ui: &mut Ui, initials: &str, color_hex: &str) {
    let size = egui::vec2(36.0, 36.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 18.0, hex_color(color_hex));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials,
        egui::FontId::proportional(14.0),
        Color32::WHITE,
    );
}

#[cfg(test)]
mod reply_tests {
    use super::*;

    #[test]
    fn reply_record_from_card_carries_raw_parent_fields() {
        let card = TimelineEventCard {
            id: "event-id".to_string(),
            author_pubkey: "author-pubkey".to_string(),
            created_at: 42,
            content: "parent".to_string(),
            ..Default::default()
        };

        let record = note_record_from_card(&card);

        assert_eq!(record.event_id, "event-id");
        assert_eq!(record.author, "author-pubkey");
        assert_eq!(record.created_at, 42);
        assert_eq!(record.content, "parent");
        assert_eq!(record.refs, Nip10Refs::default());
    }
}
